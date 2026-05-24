use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};

use axum::extract::{self, Json, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, on, MethodFilter, MethodRouter};
use axum::Router;
use serde_json::Value;
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::mysql::{MySqlPoolOptions, MySqlRow};
use sqlx::sqlite::{SqlitePoolOptions, SqliteRow};
use sqlx::{Column, MySqlPool, PgPool, Row, SqlitePool, TypeInfo};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::ast::*;
#[cfg(feature = "redis-port")]
use crate::adapter::{self as adapter_trait, FilterParam, FilterOp as AdapterFilterOp, SourceAdapter as _};
#[cfg(feature = "redis-port")]
use crate::ports::redis::RedisPort;

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Dialect {
    Postgres,
    Mysql,
    Sqlite,
}

impl Dialect {
    fn ph(&self, n: usize) -> String {
        match self {
            Dialect::Postgres => format!("${n}"),
            Dialect::Mysql | Dialect::Sqlite => "?".into(),
        }
    }

    fn returning_star(&self) -> &'static str {
        match self {
            Dialect::Postgres | Dialect::Sqlite => " RETURNING *",
            Dialect::Mysql => "",
        }
    }
}

enum DbBackend {
    Postgres(PgPool),
    Mysql(MySqlPool),
    Sqlite(SqlitePool),
}

impl DbBackend {
    fn dialect(&self) -> Dialect {
        match self {
            DbBackend::Postgres(_) => Dialect::Postgres,
            DbBackend::Mysql(_) => Dialect::Mysql,
            DbBackend::Sqlite(_) => Dialect::Sqlite,
        }
    }

    async fn fetch_optional(&self, sql: &str, params: &[Value]) -> Result<Option<Value>, FlowErr> {
        match self {
            DbBackend::Postgres(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = pg_bind(q, p); }
                q.fetch_optional(pool).await
                    .map(|r| r.map(|row| pg_row_json(&row)))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Mysql(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = my_bind(q, p); }
                q.fetch_optional(pool).await
                    .map(|r| r.map(|row| my_row_json(&row)))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Sqlite(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = sl_bind(q, p); }
                q.fetch_optional(pool).await
                    .map(|r| r.map(|row| sl_row_json(&row)))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
        }
    }

    async fn fetch_one(&self, sql: &str, params: &[Value]) -> Result<Value, FlowErr> {
        match self {
            DbBackend::Postgres(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = pg_bind(q, p); }
                q.fetch_one(pool).await
                    .map(|row| pg_row_json(&row))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Mysql(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = my_bind(q, p); }
                q.fetch_one(pool).await
                    .map(|row| my_row_json(&row))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Sqlite(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = sl_bind(q, p); }
                q.fetch_one(pool).await
                    .map(|row| sl_row_json(&row))
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
        }
    }

    async fn fetch_all(&self, sql: &str, params: &[Value]) -> Result<Vec<Value>, FlowErr> {
        match self {
            DbBackend::Postgres(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = pg_bind(q, p); }
                q.fetch_all(pool).await
                    .map(|rows| rows.iter().map(pg_row_json).collect())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Mysql(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = my_bind(q, p); }
                q.fetch_all(pool).await
                    .map(|rows| rows.iter().map(my_row_json).collect())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Sqlite(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = sl_bind(q, p); }
                q.fetch_all(pool).await
                    .map(|rows| rows.iter().map(sl_row_json).collect())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
        }
    }

    async fn count(&self, sql: &str, params: &[Value]) -> Result<i64, FlowErr> {
        match self {
            DbBackend::Postgres(pool) => {
                let mut q = sqlx::query_scalar::<_, i64>(sql);
                for p in params { q = pg_bind_scalar(q, p); }
                q.fetch_one(pool).await
                    .map_err(|e| FlowErr::Internal(format!("count: {e}")))
            }
            DbBackend::Mysql(pool) => {
                let mut q = sqlx::query_scalar::<_, i64>(sql);
                for p in params { q = my_bind_scalar(q, p); }
                q.fetch_one(pool).await
                    .map_err(|e| FlowErr::Internal(format!("count: {e}")))
            }
            DbBackend::Sqlite(pool) => {
                let mut q = sqlx::query_scalar::<_, i64>(sql);
                for p in params { q = sl_bind_scalar(q, p); }
                q.fetch_one(pool).await
                    .map_err(|e| FlowErr::Internal(format!("count: {e}")))
            }
        }
    }

    async fn execute(&self, sql: &str, params: &[Value]) -> Result<u64, FlowErr> {
        match self {
            DbBackend::Postgres(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = pg_bind(q, p); }
                q.execute(pool).await
                    .map(|r| r.rows_affected())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Mysql(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = my_bind(q, p); }
                q.execute(pool).await
                    .map(|r| r.rows_affected())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
            DbBackend::Sqlite(pool) => {
                let mut q = sqlx::query(sql);
                for p in params { q = sl_bind(q, p); }
                q.execute(pool).await
                    .map(|r| r.rows_affected())
                    .map_err(|e| FlowErr::Internal(format!("db: {e}")))
            }
        }
    }

    async fn execute_ddl(&self, sql: &str) -> Result<(), String> {
        match self {
            DbBackend::Postgres(pool) => {
                sqlx::query(sql).execute(pool).await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            DbBackend::Mysql(pool) => {
                sqlx::query(sql).execute(pool).await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            DbBackend::Sqlite(pool) => {
                sqlx::query(sql).execute(pool).await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

async fn connect_backend(url: &str, stype: SourceType) -> Result<DbBackend, Box<dyn std::error::Error>> {
    let backend = match stype {
        SourceType::Mysql => {
            DbBackend::Mysql(MySqlPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?)
        }
        SourceType::Sqlite => {
            DbBackend::Sqlite(SqlitePoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await?)
        }
        _ => {
            DbBackend::Postgres(PgPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?)
        }
    };
    Ok(backend)
}

struct AppState {
    dbs: HashMap<String, DbBackend>,
    #[cfg(feature = "redis-port")]
    redis_sources: HashMap<String, RedisPort>,
    program: Program,
    jwt_secret: String,
    rate_limiters: Mutex<HashMap<String, Vec<Instant>>>,
    cache: Mutex<HashMap<String, CacheEntry>>,
    broadcast: broadcast::Sender<Value>,
    funcs: HashMap<String, usize>,
    services: HashMap<String, usize>,
    http_client: reqwest::Client,
    adapters: Arc<tokio::sync::RwLock<AdapterRegistry>>,
    templates: HashMap<String, String>,
    locales: HashMap<String, Value>,
    default_locale: String,
    metrics: Mutex<MetricsData>,
    storages: HashMap<String, StorageConfig>,
}

struct StorageConfig {
    backend: StorageBackend,
    bucket: String,
    prefix: Option<String>,
    access: StorageAccess,
    max_size: Option<i64>,
    types: Vec<String>,
}

struct AdapterConfig {
    endpoints: HashMap<String, AdapterEndpoint>,
    config: HashMap<String, String>,
}

struct AdapterEndpoint {
    method: String,
    url: String,
}

struct AdapterRegistry {
    adapters: HashMap<String, AdapterConfig>,
}

impl AdapterRegistry {
    fn new() -> Self {
        Self { adapters: HashMap::new() }
    }

    fn load_dir(dir: &std::path::Path) -> Self {
        let mut registry = Self::new();
        if !dir.exists() {
            return registry;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return registry;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest = path.join("adapter.json");
                if manifest.exists() {
                    if let Ok(data) = std::fs::read_to_string(&manifest) {
                        if let Ok(json) = serde_json::from_str::<Value>(&data) {
                            registry.load_adapter(&json);
                        }
                    }
                }
            }
        }
        info!("loaded {} adapters", registry.adapters.len());
        registry
    }

    fn load_adapter(&mut self, json: &Value) {
        let Some(name) = json.get("name").and_then(|v| v.as_str()) else { return };
        let mut endpoints = HashMap::new();
        if let Some(eps) = json.get("endpoints").and_then(|v| v.as_object()) {
            for (k, v) in eps {
                endpoints.insert(k.clone(), AdapterEndpoint {
                    method: v.get("method").and_then(|m| m.as_str()).unwrap_or("POST").into(),
                    url: v.get("url").and_then(|u| u.as_str()).unwrap_or("").into(),
                });
            }
        }
        let mut config = HashMap::new();
        if let Some(cfg) = json.get("config").and_then(|v| v.as_object()) {
            for (k, v) in cfg {
                let env_key = v.get("env").and_then(|e| e.as_str()).unwrap_or("");
                if let Ok(val) = std::env::var(env_key) {
                    config.insert(k.clone(), val);
                }
            }
        }
        info!("adapter '{}': {} endpoints", name, endpoints.len());
        self.adapters.insert(name.into(), AdapterConfig {
            endpoints,
            config,
        });
    }

    fn get(&self, name: &str) -> Option<&AdapterConfig> {
        self.adapters.get(name)
    }

    fn reload_adapter(&mut self, dir: &std::path::Path) {
        let manifest = dir.join("adapter.json");
        if manifest.exists() {
            if let Ok(data) = std::fs::read_to_string(&manifest) {
                if let Ok(json) = serde_json::from_str::<Value>(&data) {
                    let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    self.load_adapter(&json);
                    info!("hot-reloaded adapter '{}'", name);
                }
            }
        }
    }
}

struct CacheEntry {
    value: Value,
    expires: Instant,
}

#[derive(Default)]
struct MetricsData {
    request_total: HashMap<String, u64>,
    error_total: HashMap<String, u64>,
    latency_sum: HashMap<String, f64>,
    latency_count: HashMap<String, u64>,
}

impl MetricsData {
    fn record(&mut self, flow: &str, method: &str, status: u16, duration_secs: f64) {
        let key = format!("{}:{}", flow, method);
        *self.request_total.entry(format!("{key}:{status}")).or_default() += 1;
        if status >= 400 {
            *self.error_total.entry(key.clone()).or_default() += 1;
        }
        *self.latency_sum.entry(key.clone()).or_default() += duration_secs;
        *self.latency_count.entry(key).or_default() += 1;
    }

    fn render_prometheus(&self) -> String {
        let mut out = String::new();

        writeln!(out, "# HELP axis_request_total Total HTTP requests per flow/method/status").unwrap();
        writeln!(out, "# TYPE axis_request_total counter").unwrap();
        let mut keys: Vec<_> = self.request_total.keys().collect();
        keys.sort();
        for key in keys {
            let count = self.request_total[key];
            let parts: Vec<&str> = key.splitn(3, ':').collect();
            if parts.len() == 3 {
                writeln!(out, "axis_request_total{{flow=\"{}\",method=\"{}\",status=\"{}\"}} {count}",
                    parts[0], parts[1], parts[2]).unwrap();
            }
        }

        writeln!(out).unwrap();
        writeln!(out, "# HELP axis_error_total Total error responses per flow/method").unwrap();
        writeln!(out, "# TYPE axis_error_total counter").unwrap();
        let mut keys: Vec<_> = self.error_total.keys().collect();
        keys.sort();
        for key in keys {
            let count = self.error_total[key];
            let parts: Vec<&str> = key.splitn(2, ':').collect();
            if parts.len() == 2 {
                writeln!(out, "axis_error_total{{flow=\"{}\",method=\"{}\"}} {count}",
                    parts[0], parts[1]).unwrap();
            }
        }

        writeln!(out).unwrap();
        writeln!(out, "# HELP axis_request_duration_seconds Total request duration per flow/method").unwrap();
        writeln!(out, "# TYPE axis_request_duration_seconds summary").unwrap();
        let mut keys: Vec<_> = self.latency_sum.keys().collect();
        keys.sort();
        for key in keys {
            let sum = self.latency_sum[key];
            let count = self.latency_count.get(key).copied().unwrap_or(0);
            let parts: Vec<&str> = key.splitn(2, ':').collect();
            if parts.len() == 2 {
                writeln!(out, "axis_request_duration_seconds_sum{{flow=\"{}\",method=\"{}\"}} {sum:.6}",
                    parts[0], parts[1]).unwrap();
                writeln!(out, "axis_request_duration_seconds_count{{flow=\"{}\",method=\"{}\"}} {count}",
                    parts[0], parts[1]).unwrap();
            }
        }

        out
    }
}

struct Ctx {
    path: HashMap<String, String>,
    query: HashMap<String, String>,
    body: Value,
    headers: HeaderMap,
    bindings: HashMap<String, Value>,
    claims: Value,
    tenant: Option<(String, Value)>,
}

enum FlowErr {
    Http(StatusCode, String),
    Internal(String),
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

pub struct ServeOpts {
    pub src_dir: Option<String>,
    pub adapters_dir: Option<String>,
    pub templates_dir: Option<String>,
    pub locales_dir: Option<String>,
    pub db_url: Option<String>,
    pub jwt_secret: Option<String>,
}

fn init_logging() {
    use tracing_subscriber::EnvFilter;

    let level = std::env::var("AXIS_LOG_LEVEL").unwrap_or_else(|_| "info".into());
    let channels = std::env::var("AXIS_LOG_CHANNELS").unwrap_or_else(|_| String::new());

    let mut filter = format!("axis={level}");

    if !channels.is_empty() {
        let base_suppress = "axis=warn";
        let channel_directives: Vec<String> = channels
            .split(',')
            .map(|ch| ch.trim())
            .filter(|ch| !ch.is_empty())
            .map(|ch| format!("axis::{}={}", ch, level))
            .collect();
        if !channel_directives.is_empty() {
            filter = format!("{},{}", base_suppress, channel_directives.join(","));
        }
    }

    let env_filter = EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init();
}

pub async fn serve(dir: &Path, port: u16, opts: ServeOpts) -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let compile_dir_buf = opts.src_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let src = dir.join("src");
            if src.exists() { src } else { dir.to_path_buf() }
        });
    let result = crate::project::compile_project(&compile_dir_buf);
    for e in &result.errors {
        eprintln!("ERROR [{}]: {}", e.file.display(), e.error);
    }
    if !result.is_ok() {
        return Err("compilation failed".into());
    }

    let verifier = crate::verify::Verifier::new();
    let vr = verifier.verify(&result.program);
    for e in &vr.errors {
        eprintln!("ERROR: {e}");
    }
    if vr.error_count() > 0 {
        return Err("verification failed".into());
    }

    let needs_auth = result.program.constructs.iter().any(|c| {
        matches!(c, Construct::Flow(f) if f.auth.is_some())
            || matches!(c, Construct::Saga(s) if s.auth.is_some())
    });

    let sql_sources: Vec<(&str, SourceType)> = result.program.constructs.iter().filter_map(|c| {
        if let Construct::Source(s) = c {
            match s.source_type {
                SourceType::Postgres | SourceType::Mysql | SourceType::Sqlite => Some((s.name.as_str(), s.source_type)),
                _ => None,
            }
        } else {
            None
        }
    }).collect();

    let default_url = opts.db_url.unwrap_or_default();
    let mut dbs: HashMap<String, DbBackend> = HashMap::new();

    for (name, stype) in &sql_sources {
        let env_key = format!("{}_DATABASE_URL", name.to_uppercase());
        let url = std::env::var(&env_key).unwrap_or_else(|_| default_url.clone());
        if url.is_empty() {
            continue;
        }
        let backend = connect_backend(&url, *stype).await?;
        dbs.insert(name.to_string(), backend);
    }

    #[cfg(feature = "redis-port")]
    let redis_sources = {
        let mut rs: HashMap<String, RedisPort> = HashMap::new();
        for c in &result.program.constructs {
            if let Construct::Source(s) = c {
                if s.source_type == SourceType::Redis {
                    let env_key = format!("{}_REDIS_URL", s.name.to_uppercase());
                    let url = std::env::var(&env_key)
                        .or_else(|_| std::env::var("REDIS_URL"))
                        .unwrap_or_else(|_| "redis://127.0.0.1/".into());
                    let prefix = std::env::var(format!("{}_REDIS_PREFIX", s.name.to_uppercase()))
                        .unwrap_or_default();
                    match RedisPort::connect(&url, &prefix).await {
                        Ok(adapter) => {
                            info!("connected redis source '{}'", s.name);
                            rs.insert(s.name.clone(), adapter);
                        }
                        Err(e) => eprintln!("redis source '{}': {e}", s.name),
                    }
                }
            }
        }
        rs
    };

    if std::env::var("AXIS_AUTO_SCHEMA").is_ok() {
        if let Err(e) = auto_create_tables(&result.program, &dbs).await {
            eprintln!("schema sync: {e}");
        }
    }

    let jwt_secret = if needs_auth {
        opts.jwt_secret.unwrap_or_else(|| "axis-dev-secret".into())
    } else {
        opts.jwt_secret.unwrap_or_default()
    };
    let (tx, _) = broadcast::channel::<Value>(1024);

    let mut funcs = HashMap::new();
    let mut services = HashMap::new();
    for (i, c) in result.program.constructs.iter().enumerate() {
        if let Construct::Func(f) = c {
            funcs.insert(f.name.clone(), i);
        }
        if let Construct::Service(s) = c {
            services.insert(s.name.clone(), i);
        }
    }

    let http_client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .build()
        .expect("failed to build HTTP client");

    let adapters_dir = opts.adapters_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.join("adapters"));
    let adapter_registry = AdapterRegistry::load_dir(&adapters_dir);
    let adapters = Arc::new(tokio::sync::RwLock::new(adapter_registry));

    let templates_dir = opts.templates_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.join("templates"));
    let locales_dir = opts.locales_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.join("locales"));
    let templates = load_templates(&templates_dir);
    let (locales, default_locale) = load_locales(&locales_dir);

    let mut storages = HashMap::new();
    for c in &result.program.constructs {
        if let Construct::Storage(s) = c {
            storages.insert(s.name.clone(), StorageConfig {
                backend: s.backend.clone(),
                bucket: s.bucket.clone(),
                prefix: s.prefix.clone(),
                access: s.access.clone(),
                max_size: s.max_size,
                types: s.types.clone(),
            });
        }
    }

    let state = Arc::new(AppState {
        dbs,
        #[cfg(feature = "redis-port")]
        redis_sources,
        program: result.program,
        jwt_secret,
        rate_limiters: Mutex::new(HashMap::new()),
        cache: Mutex::new(HashMap::new()),
        broadcast: tx,
        funcs,
        services,
        http_client,
        adapters: adapters.clone(),
        templates,
        locales,
        default_locale,
        metrics: Mutex::new(MetricsData::default()),
        storages,
    });

    // hot-swap watcher for adapters directory
    if adapters_dir.exists() {
        let watch_adapters = adapters.clone();
        let watch_dir = adapters_dir.clone();
        tokio::spawn(async move {
            adapter_watcher(watch_dir, watch_adapters).await;
        });
    }

    // collect routes from FLOWs and SAGAs
    let mut path_groups: HashMap<String, Vec<(usize, MethodFilter)>> = HashMap::new();
    for (i, c) in state.program.constructs.iter().enumerate() {
        match c {
            Construct::Flow(flow) => {
                let path = axum_path(&flow.path);
                let method = to_method_filter(&flow.method);
                path_groups.entry(path).or_default().push((i, method));
            }
            Construct::Saga(saga) => {
                let path = axum_path(&saga.path);
                let method = to_method_filter(&saga.method);
                path_groups.entry(path).or_default().push((i, method));
            }
            _ => {}
        }
    }

    // add SURFACE aliases
    let mut name_idx: HashMap<String, usize> = HashMap::new();
    for (i, c) in state.program.constructs.iter().enumerate() {
        match c {
            Construct::Flow(f) => {
                name_idx.insert(f.name.clone(), i);
            }
            Construct::Saga(s) => {
                name_idx.insert(s.name.clone(), i);
            }
            _ => {}
        }
    }
    for c in state.program.constructs.iter() {
        if let Construct::Surface(surface) = c {
            let base = surface.base_path.as_deref().unwrap_or("");
            for route in &surface.routes {
                if let Some(&idx) = name_idx.get(&route.target) {
                    let path = format!("{}{}", base, axum_path(&route.path));
                    let method = to_method_filter(&route.method);
                    path_groups.entry(path).or_default().push((idx, method));
                }
            }
        }
    }

    let mut router = Router::new();
    router = router.route("/metrics", get(metrics_handler));

    for (path, group) in &path_groups {
        let has_path_params = path.contains('{');
        let mut mr: Option<MethodRouter<Arc<AppState>>> = None;

        for &(idx, method) in group {
            let new_mr = if has_path_params {
                let h = move |State(st): State<Arc<AppState>>,
                              headers: HeaderMap,
                              extract::Path(pp): extract::Path<HashMap<String, String>>,
                              Query(qp): Query<HashMap<String, String>>,
                              body: Option<Json<Value>>| async move {
                    handle_route(&st, idx, pp, qp, body.map(|b| b.0), headers).await
                };
                match mr {
                    Some(m) => m.on(method, h),
                    None => on(method, h),
                }
            } else {
                let h = move |State(st): State<Arc<AppState>>,
                              headers: HeaderMap,
                              Query(qp): Query<HashMap<String, String>>,
                              body: Option<Json<Value>>| async move {
                    handle_route(&st, idx, HashMap::new(), qp, body.map(|b| b.0), headers).await
                };
                match mr {
                    Some(m) => m.on(method, h),
                    None => on(method, h),
                }
            };
            mr = Some(new_mr);
        }

        router = router.route(path, mr.unwrap());
    }

    // register STREAM routes
    for (i, c) in state.program.constructs.iter().enumerate() {
        if let Construct::Stream(stream) = c {
            let path = axum_path(&stream.path);
            let idx = i;
            match stream.transport {
                StreamTransport::WebSocket => {
                    router = router.route(
                        &path,
                        get(move |State(st): State<Arc<AppState>>,
                                  headers: HeaderMap,
                                  ws: axum::extract::ws::WebSocketUpgrade| async move {
                            ws_upgrade(st, headers, ws, idx).await
                        }),
                    );
                }
                StreamTransport::Sse => {
                    router = router.route(
                        &path,
                        get(
                            move |State(st): State<Arc<AppState>>,
                                  headers: HeaderMap| async move {
                                sse_handler(st, headers, idx).await
                            },
                        ),
                    );
                }
            }
        }
    }

    for (name, cfg) in &state.storages {
        if cfg.access == StorageAccess::Public && cfg.backend == StorageBackend::Local {
            let serve_path = format!("/files/{}", cfg.prefix.as_deref().unwrap_or(&cfg.bucket));
            let dir = std::path::Path::new(&cfg.bucket).join(cfg.prefix.as_deref().unwrap_or(""));
            if dir.exists() {
                info!(storage = %name, path = %serve_path, "serving public files");
            }
            router = router.nest_service(
                &serve_path,
                tower_http::services::ServeDir::new(&dir),
            );
        }
    }

    let app = build_app(router, state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(target: "axis::lifecycle", %addr, "axis serving");
    eprintln!("axis serving on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_app(
    router: Router<Arc<AppState>>,
    state: Arc<AppState>,
) -> Router {
    router.layer(CorsLayer::permissive()).with_state(state)
}

pub async fn build_app_from_source(
    source: &str,
    db_url: &str,
) -> Result<Router, Box<dyn std::error::Error>> {
    let program = crate::project::compile_source(source)?;

    let verifier = crate::verify::Verifier::new();
    let vr = verifier.verify(&program);
    if vr.error_count() > 0 {
        return Err(format!("verification: {} errors", vr.error_count()).into());
    }

    let sql_sources: Vec<(&str, SourceType)> = program.constructs.iter().filter_map(|c| {
        if let Construct::Source(s) = c {
            match s.source_type {
                SourceType::Postgres | SourceType::Mysql | SourceType::Sqlite => Some((s.name.as_str(), s.source_type)),
                _ => None,
            }
        } else {
            None
        }
    }).collect();

    let mut dbs: HashMap<String, DbBackend> = HashMap::new();
    for (name, stype) in &sql_sources {
        let env_key = format!("{}_DATABASE_URL", name.to_uppercase());
        let url = std::env::var(&env_key).unwrap_or_else(|_| db_url.to_string());
        let backend = connect_backend(&url, *stype).await?;
        dbs.insert(name.to_string(), backend);
    }

    auto_create_tables(&program, &dbs).await?;

    let jwt_secret = "test-secret".to_string();
    let (tx, _) = broadcast::channel::<Value>(1024);

    let mut funcs = HashMap::new();
    let mut services = HashMap::new();
    for (i, c) in program.constructs.iter().enumerate() {
        if let Construct::Func(f) = c { funcs.insert(f.name.clone(), i); }
        if let Construct::Service(s) = c { services.insert(s.name.clone(), i); }
    }

    let mut test_storages = HashMap::new();
    for c in &program.constructs {
        if let Construct::Storage(s) = c {
            test_storages.insert(s.name.clone(), StorageConfig {
                backend: s.backend.clone(),
                bucket: s.bucket.clone(),
                prefix: s.prefix.clone(),
                access: s.access.clone(),
                max_size: s.max_size,
                types: s.types.clone(),
            });
        }
    }

    let state = Arc::new(AppState {
        dbs,
        #[cfg(feature = "redis-port")]
        redis_sources: HashMap::new(),
        program,
        jwt_secret,
        rate_limiters: Mutex::new(HashMap::new()),
        cache: Mutex::new(HashMap::new()),
        broadcast: tx,
        funcs,
        services,
        http_client: reqwest::Client::new(),
        adapters: Arc::new(tokio::sync::RwLock::new(AdapterRegistry { adapters: HashMap::new() })),
        templates: HashMap::new(),
        locales: HashMap::new(),
        default_locale: "en".into(),
        metrics: Mutex::new(MetricsData::default()),
        storages: test_storages,
    });

    let mut path_groups: HashMap<String, Vec<(usize, MethodFilter)>> = HashMap::new();
    for (i, c) in state.program.constructs.iter().enumerate() {
        match c {
            Construct::Flow(flow) => {
                let path = axum_path(&flow.path);
                let method = to_method_filter(&flow.method);
                path_groups.entry(path).or_default().push((i, method));
            }
            Construct::Saga(saga) => {
                let path = axum_path(&saga.path);
                let method = to_method_filter(&saga.method);
                path_groups.entry(path).or_default().push((i, method));
            }
            _ => {}
        }
    }

    let mut router = Router::new();
    router = router.route("/metrics", get(metrics_handler));

    for (path, group) in &path_groups {
        let has_path_params = path.contains('{');
        let mut mr: Option<MethodRouter<Arc<AppState>>> = None;
        for &(idx, method) in group {
            let new_mr = if has_path_params {
                let h = move |State(st): State<Arc<AppState>>,
                              headers: HeaderMap,
                              extract::Path(pp): extract::Path<HashMap<String, String>>,
                              Query(qp): Query<HashMap<String, String>>,
                              body: Option<Json<Value>>| async move {
                    handle_route(&st, idx, pp, qp, body.map(|b| b.0), headers).await
                };
                match mr { Some(m) => m.on(method, h), None => on(method, h) }
            } else {
                let h = move |State(st): State<Arc<AppState>>,
                              headers: HeaderMap,
                              Query(qp): Query<HashMap<String, String>>,
                              body: Option<Json<Value>>| async move {
                    handle_route(&st, idx, HashMap::new(), qp, body.map(|b| b.0), headers).await
                };
                match mr { Some(m) => m.on(method, h), None => on(method, h) }
            };
            mr = Some(new_mr);
        }
        router = router.route(path, mr.unwrap());
    }

    Ok(build_app(router, state))
}

// ---------------------------------------------------------------------------
// route helpers
// ---------------------------------------------------------------------------

fn axum_path(path: &str) -> String {
    path.split('/')
        .map(|s| {
            if let Some(name) = s.strip_prefix(':') {
                format!("{{{name}}}")
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn to_method_filter(m: &HttpMethod) -> MethodFilter {
    match m {
        HttpMethod::Get => MethodFilter::GET,
        HttpMethod::Post => MethodFilter::POST,
        HttpMethod::Put => MethodFilter::PUT,
        HttpMethod::Patch => MethodFilter::PATCH,
        HttpMethod::Delete => MethodFilter::DELETE,
        HttpMethod::Webhook => MethodFilter::POST,
    }
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let data = state.metrics.lock().unwrap();
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        data.render_prometheus(),
    )
}

fn to_std_duration(d: &Duration) -> StdDuration {
    let millis = match d.unit {
        DurationUnit::Milliseconds => d.value as u64,
        DurationUnit::Seconds => d.value as u64 * 1000,
        DurationUnit::Minutes => d.value as u64 * 60_000,
        DurationUnit::Hours => d.value as u64 * 3_600_000,
    };
    StdDuration::from_millis(millis)
}

// ---------------------------------------------------------------------------
// auth
// ---------------------------------------------------------------------------

fn check_auth(
    auth: &AuthDecl,
    headers: &HeaderMap,
    jwt_secret: &str,
    body: &Value,
) -> Result<Value, FlowErr> {
    tracing::debug!(target: "axis::auth", method = ?auth, "checking auth");
    match auth {
        AuthDecl::None => Ok(Value::Null),
        AuthDecl::Session | AuthDecl::Bearer => {
            let token = extract_bearer(headers)?;
            decode_jwt(&token, jwt_secret)
        }
        AuthDecl::ApiKey => {
            let key = headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    FlowErr::Http(StatusCode::UNAUTHORIZED, "missing api key".into())
                })?;
            Ok(serde_json::json!({ "api_key": key }))
        }
        AuthDecl::Role(role) => {
            let token = extract_bearer(headers)?;
            let claims = decode_jwt(&token, jwt_secret)?;
            let user_role = claims
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if user_role == role {
                Ok(claims)
            } else {
                Err(FlowErr::Http(
                    StatusCode::FORBIDDEN,
                    "insufficient role".into(),
                ))
            }
        }
        AuthDecl::RoleIn(roles) => {
            let token = extract_bearer(headers)?;
            let claims = decode_jwt(&token, jwt_secret)?;
            let user_role = claims
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if roles.iter().any(|r| r == user_role) {
                Ok(claims)
            } else {
                Err(FlowErr::Http(
                    StatusCode::FORBIDDEN,
                    "insufficient role".into(),
                ))
            }
        }
        AuthDecl::WebhookSignature { secret, .. } => {
            let body_bytes = serde_json::to_vec(body).unwrap_or_default();
            verify_webhook(headers, &body_bytes, secret)
        }
    }
}

fn extract_bearer(headers: &HeaderMap) -> Result<String, FlowErr> {
    let val = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| FlowErr::Http(StatusCode::UNAUTHORIZED, "missing authorization".into()))?;
    val.strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or_else(|| FlowErr::Http(StatusCode::UNAUTHORIZED, "invalid bearer token".into()))
}

fn decode_jwt(token: &str, secret: &str) -> Result<Value, FlowErr> {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    let key = DecodingKey::from_secret(secret.as_bytes());
    let validation = Validation::new(Algorithm::HS256);
    let data = decode::<HashMap<String, Value>>(token, &key, &validation)
        .map_err(|e| FlowErr::Http(StatusCode::UNAUTHORIZED, format!("invalid token: {e}")))?;
    serde_json::to_value(data.claims)
        .map_err(|e| FlowErr::Internal(format!("claims: {e}")))
}

fn verify_webhook(
    headers: &HeaderMap,
    body_bytes: &[u8],
    secret: &str,
) -> Result<Value, FlowErr> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let sig_header = headers
        .get("x-signature")
        .or_else(|| headers.get("x-hub-signature-256"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| FlowErr::Http(StatusCode::UNAUTHORIZED, "missing signature".into()))?;

    let sig_hex = sig_header.strip_prefix("sha256=").unwrap_or(sig_header);

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| FlowErr::Internal("invalid hmac key".into()))?;
    mac.update(body_bytes);
    let expected = hex::encode(mac.finalize().into_bytes());

    if expected == sig_hex {
        Ok(Value::Null)
    } else {
        Err(FlowErr::Http(
            StatusCode::UNAUTHORIZED,
            "invalid signature".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// rate limiting
// ---------------------------------------------------------------------------

fn check_rate_limit(
    limiters: &Mutex<HashMap<String, Vec<Instant>>>,
    key: &str,
    limit: &LimitDecl,
) -> Result<(), FlowErr> {
    let window = match limit.unit {
        RateUnit::PerSecond => StdDuration::from_secs(1),
        RateUnit::PerMinute => StdDuration::from_secs(60),
        RateUnit::PerHour => StdDuration::from_secs(3600),
        RateUnit::PerDay => StdDuration::from_secs(86400),
    };
    let now = Instant::now();
    let mut map = limiters.lock().unwrap();
    let stamps = map.entry(key.to_string()).or_default();
    stamps.retain(|t| now.duration_since(*t) < window);
    if stamps.len() >= limit.count as usize {
        Err(FlowErr::Http(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded".into(),
        ))
    } else {
        stamps.push(now);
        Ok(())
    }
}

fn rate_key(flow_name: &str, limit: &LimitDecl, ctx: &Ctx) -> String {
    let scope_id = match &limit.scope {
        RateScope::PerUser => ctx
            .claims
            .get("sub")
            .or_else(|| ctx.claims.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("anon")
            .to_string(),
        RateScope::PerIp => ctx
            .headers
            .get("x-forwarded-for")
            .or_else(|| ctx.headers.get("x-real-ip"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        RateScope::PerKey => ctx
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        RateScope::Global => "global".to_string(),
    };
    format!("{flow_name}:{scope_id}")
}

// ---------------------------------------------------------------------------
// route dispatch
// ---------------------------------------------------------------------------

async fn handle_route(
    state: &AppState,
    idx: usize,
    path_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    body: Option<Value>,
    headers: HeaderMap,
) -> Response {
    let start = Instant::now();
    let (flow_name, method_str, flow_path) = match &state.program.constructs[idx] {
        Construct::Flow(f) => (f.name.clone(), format!("{:?}", f.method), f.path.clone()),
        Construct::Saga(s) => (s.name.clone(), format!("{:?}", s.method), s.path.clone()),
        _ => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, "invalid route target"),
    };

    tracing::info!(target: "axis::request", method = %method_str, path = %flow_path, flow = %flow_name, "request");

    let resp = match &state.program.constructs[idx] {
        Construct::Flow(_) => {
            run_flow(state, idx, path_params, query_params, body, headers).await
        }
        Construct::Saga(_) => {
            run_saga(state, idx, path_params, query_params, body, headers).await
        }
        _ => unreachable!(),
    };

    let duration = start.elapsed().as_secs_f64();
    let status = resp.status().as_u16();
    tracing::info!(target: "axis::request", method = %method_str, path = %flow_path, flow = %flow_name, status, duration_ms = format!("{:.1}", duration * 1000.0), "response");
    if let Ok(mut m) = state.metrics.lock() {
        m.record(&flow_name, &method_str, status, duration);
    }

    resp
}

// ---------------------------------------------------------------------------
// run flow
// ---------------------------------------------------------------------------

async fn run_flow(
    state: &AppState,
    flow_idx: usize,
    path_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    body: Option<Value>,
    headers: HeaderMap,
) -> Response {
    let flow = match &state.program.constructs[flow_idx] {
        Construct::Flow(f) => f,
        _ => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, "invalid flow"),
    };

    let body_val = body.unwrap_or(Value::Null);

    let mut ctx = Ctx {
        path: path_params,
        query: query_params,
        body: body_val.clone(),
        headers,
        bindings: HashMap::new(),
        claims: Value::Null,
        tenant: None,
    };

    // auth
    if let Some(auth) = &flow.auth {
        match check_auth(auth, &ctx.headers, &state.jwt_secret, &body_val) {
            Ok(claims) => ctx.claims = claims,
            Err(e) => return flow_err_resp(e),
        }
    }

    // rate limiting
    for limit in &flow.limits {
        let key = rate_key(&flow.name, limit, &ctx);
        if let Err(e) = check_rate_limit(&state.rate_limiters, &key, limit) {
            return flow_err_resp(e);
        }
    }

    // response cache (GET only)
    let cache_key = if !flow.cache.is_empty() && flow.method == HttpMethod::Get {
        let vary_parts: Vec<String> = flow
            .cache
            .iter()
            .flat_map(|cd| cd.vary.iter())
            .map(|dp| format!("{:?}", resolve_dot(dp, &ctx)))
            .collect();
        let key = format!("{}:{:?}:{:?}:{}", flow.name, ctx.path, ctx.query, vary_parts.join(","));
        if let Some(entry) = state.cache.lock().unwrap().get(&key) {
            if entry.expires > Instant::now() {
                tracing::debug!(target: "axis::cache", flow = %flow.name, "cache hit");
                let status =
                    StatusCode::from_u16(flow.return_stmt.code as u16).unwrap_or(StatusCode::OK);
                return (status, axum::Json(entry.value.clone())).into_response();
            }
        }
        Some(key)
    } else {
        None
    };

    // scope / tenant
    if let Some(ScopeDecl::Tenant(dp)) = &flow.scope {
        let tenant_val = resolve_dot(dp, &ctx);
        let tenant_field = find_realm_tenant(&state.program, flow.realm.as_deref());
        if let Some(field) = tenant_field {
            ctx.tenant = Some((field, tenant_val));
        }
    }

    // execute steps (with optional timeout)
    let step_result = if let Some(timeout) = &flow.timeout {
        let dur = to_std_duration(timeout);
        match tokio::time::timeout(dur, exec_steps(state, &flow.steps, &mut ctx)).await {
            Ok(r) => r,
            Err(_) => {
                return err_resp(StatusCode::GATEWAY_TIMEOUT, "timeout");
            }
        }
    } else {
        exec_steps(state, &flow.steps, &mut ctx).await
    };

    match step_result {
        Ok(()) => {}
        Err(FlowErr::Http(code, msg)) => return err_resp(code, &msg),
        Err(FlowErr::Internal(msg)) => {
            eprintln!("flow {}: {msg}", flow.name);
            return err_resp(StatusCode::INTERNAL_SERVER_ERROR, &msg);
        }
    }

    let resp = build_return_resp(&flow.return_stmt, &ctx);

    // store in cache
    if let Some(key) = cache_key {
        if let Some(cd) = flow.cache.first() {
            let ttl = StdDuration::from_secs(cd.ttl as u64);
            // extract body from response for caching — build it again
            let val = return_body_value(&flow.return_stmt, &ctx);
            state.cache.lock().unwrap().insert(
                key,
                CacheEntry {
                    value: val,
                    expires: Instant::now() + ttl,
                },
            );
        }
    }

    resp
}

// ---------------------------------------------------------------------------
// run saga
// ---------------------------------------------------------------------------

async fn run_saga(
    state: &AppState,
    saga_idx: usize,
    path_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    body: Option<Value>,
    headers: HeaderMap,
) -> Response {
    let saga = match &state.program.constructs[saga_idx] {
        Construct::Saga(s) => s,
        _ => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, "invalid saga"),
    };

    let body_val = body.unwrap_or(Value::Null);
    let mut ctx = Ctx {
        path: path_params,
        query: query_params,
        body: body_val.clone(),
        headers,
        bindings: HashMap::new(),
        claims: Value::Null,
        tenant: None,
    };

    if let Some(auth) = &saga.auth {
        match check_auth(auth, &ctx.headers, &state.jwt_secret, &body_val) {
            Ok(claims) => ctx.claims = claims,
            Err(e) => return flow_err_resp(e),
        }
    }

    let mut completed: Vec<usize> = Vec::new();

    for (i, step) in saga.steps.iter().enumerate() {
        let step_ok = exec_steps(state, &step.flow_steps, &mut ctx).await.is_ok();

        let verify_ok = if step_ok {
            if let Some(verify_expr) = &step.verify {
                eval_expr(state, verify_expr, &mut ctx)
                    .await
                    .map(|v| is_truthy(&v))
                    .unwrap_or(false)
            } else {
                true
            }
        } else {
            false
        };

        if !step_ok || !verify_ok {
            if saga.on_failure.run_compensations {
                for &ci in completed.iter().rev() {
                    if let Compensate::Steps(comp) = &saga.steps[ci].compensate {
                        let _ = exec_steps(state, comp, &mut ctx).await;
                    }
                }
            }
            return err_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("saga step {} failed", step.name),
            );
        }

        completed.push(i);
    }

    // on_success effects
    for effect in &saga.on_success.effects {
        exec_effect(state, effect, &ctx);
    }

    build_return_resp(&saga.on_success.return_stmt, &ctx)
}

// ---------------------------------------------------------------------------
// stream handlers
// ---------------------------------------------------------------------------

async fn ws_upgrade(
    state: Arc<AppState>,
    headers: HeaderMap,
    ws: axum::extract::ws::WebSocketUpgrade,
    stream_idx: usize,
) -> Response {
    let stream = match &state.program.constructs[stream_idx] {
        Construct::Stream(s) => s,
        _ => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, "invalid stream"),
    };
    if let Some(auth) = &stream.auth {
        if let Err(e) = check_auth(auth, &headers, &state.jwt_secret, &Value::Null) {
            return flow_err_resp(e);
        }
    }
    ws.on_upgrade(move |socket| handle_ws(state, socket, stream_idx))
}

async fn handle_ws(
    state: Arc<AppState>,
    mut socket: axum::extract::ws::WebSocket,
    stream_idx: usize,
) {
    let stream = match &state.program.constructs[stream_idx] {
        Construct::Stream(s) => s.clone(),
        _ => return,
    };

    let mut rx = state.broadcast.subscribe();
    let event_names: Vec<String> = stream.events.iter().map(|e| e.name.clone()).collect();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(v) => {
                        if let Some(name) = v.get("event").and_then(|e| e.as_str()) {
                            if event_names.iter().any(|n| n == name) {
                                let text = serde_json::to_string(&v).unwrap_or_default();
                                if socket.send(axum::extract::ws::Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            if let Some(event_name) = v.get("event").and_then(|e| e.as_str()) {
                                for receiver in &stream.receivers {
                                    if receiver.event == event_name {
                                        let mut ctx = Ctx {
                                            path: HashMap::new(),
                                            query: HashMap::new(),
                                            body: v.clone(),
                                            headers: HeaderMap::new(),
                                            bindings: HashMap::new(),
                                            claims: Value::Null,
                                            tenant: None,
                                        };
                                        let _ = exec_steps(&state, &receiver.steps, &mut ctx).await;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

async fn sse_handler(state: Arc<AppState>, headers: HeaderMap, stream_idx: usize) -> Response {
    let stream_def = match &state.program.constructs[stream_idx] {
        Construct::Stream(s) => s,
        _ => return err_resp(StatusCode::INTERNAL_SERVER_ERROR, "invalid stream"),
    };
    if let Some(auth) = &stream_def.auth {
        if let Err(e) = check_auth(auth, &headers, &state.jwt_secret, &Value::Null) {
            return flow_err_resp(e);
        }
    }

    let event_names: Vec<String> = stream_def.events.iter().map(|e| e.name.clone()).collect();
    let rx = state.broadcast.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(v) => {
            if let Some(name) = v.get("event").and_then(|e| e.as_str()) {
                if event_names.iter().any(|n| n == name) {
                    let data = serde_json::to_string(&v).unwrap_or_default();
                    Some(Ok::<_, std::convert::Infallible>(
                        Event::default().event(name).data(data),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Err(_) => None,
    });

    Sse::new(stream).into_response()
}

// ---------------------------------------------------------------------------
// step execution
// ---------------------------------------------------------------------------

async fn exec_steps(state: &AppState, steps: &[FlowStep], ctx: &mut Ctx) -> Result<(), FlowErr> {
    for step in steps {
        exec_step(state, step, ctx).await?;
    }
    Ok(())
}

fn exec_step<'a>(
    state: &'a AppState,
    step: &'a FlowStep,
    ctx: &'a mut Ctx,
) -> Pin<Box<dyn Future<Output = Result<(), FlowErr>> + Send + 'a>> {
    Box::pin(async move {
        match step {
            FlowStep::Let(s) => {
                let val = eval_expr(state, &s.expr, &mut *ctx).await?;
                ctx.bindings.insert(s.name.clone(), val);
            }
            FlowStep::Set(s) => {
                let val = eval_expr(state, &s.expr, &mut *ctx).await?;
                ctx.bindings.insert(s.name.clone(), val);
            }
            FlowStep::Insert(s) => exec_insert(state, s, &mut *ctx).await?,
            FlowStep::Update(s) => exec_update(state, s, &mut *ctx).await?,
            FlowStep::Delete(s) => exec_delete(state, s, &mut *ctx).await?,
            FlowStep::Rule(rule) => {
                for req in &rule.requires {
                    let left = resolve_dot(&req.path, ctx);
                    let right = eval_expr(state, &req.value, &mut *ctx).await?;
                    if !compare_values(&left, &req.op, &right) {
                        return Err(FlowErr::Http(
                            StatusCode::FORBIDDEN,
                            format!("rule {} failed", rule.name),
                        ));
                    }
                }
            }
            FlowStep::Guard(guard) => {
                let val = eval_expr(state, &guard.expr, &mut *ctx).await?;
                if !is_truthy(&val) {
                    let status = StatusCode::from_u16(guard.code as u16)
                        .unwrap_or(StatusCode::BAD_REQUEST);
                    let msg = guard.message.as_deref().unwrap_or("guard failed");
                    return Err(FlowErr::Http(status, msg.into()));
                }
            }
            FlowStep::Effect(effect) => {
                exec_effect(state, effect, ctx);
            }
            FlowStep::Match(m) => {
                let mut matched = false;
                for branch in &m.branches {
                    let val = eval_expr(state, &branch.condition, &mut *ctx).await?;
                    if is_truthy(&val) {
                        exec_steps(state, &branch.steps, &mut *ctx).await?;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    if let Some(default) = &m.default {
                        exec_steps(state, default, &mut *ctx).await?;
                    }
                }
            }
            FlowStep::Each(each) => {
                let source_val = eval_expr(state, &each.source, &mut *ctx).await?;
                if let Value::Array(items) = source_val {
                    for item in items {
                        ctx.bindings.insert(each.binding.clone(), item);
                        exec_steps(state, &each.steps, &mut *ctx).await?;
                    }
                    ctx.bindings.remove(&each.binding);
                }
            }
            FlowStep::Try(t) => {
                let body_result = exec_steps(state, &t.body, &mut *ctx).await;
                if body_result.is_err() {
                    exec_steps(state, &t.recover, &mut *ctx).await?;
                }
            }
            FlowStep::Upload(upload) => {
                let file_val = eval_expr(state, &upload.file_expr, &mut *ctx).await?;
                let url = exec_upload(state, &upload.storage, &file_val).await?;
                ctx.bindings.insert(upload.binding.clone(), Value::String(url));
            }
        }
        Ok(())
    })
}

fn exec_effect(state: &AppState, effect: &EffectStep, ctx: &Ctx) {
    let mut data = serde_json::Map::new();
    let kind = match &effect.kind {
        EffectKind::Email => "email",
        EffectKind::PushNotification => "push",
        EffectKind::Async => "async",
        EffectKind::Webhook => "webhook",
    };
    data.insert("kind".into(), Value::String(kind.into()));
    for field in &effect.fields {
        match field {
            EffectField::Template(t) => {
                data.insert("template".into(), Value::String(t.clone()));
            }
            EffectField::To(expr) => {
                data.insert("to".into(), resolve_val(expr, ctx));
            }
            EffectField::Event(e) => {
                data.insert("event".into(), Value::String(e.clone()));
            }
            EffectField::Task(t) => {
                data.insert("task".into(), Value::String(t.clone()));
            }
            EffectField::Url(expr) => {
                data.insert("url".into(), resolve_val(expr, ctx));
            }
            EffectField::Data(exprs) => {
                let vals: Vec<Value> = exprs.iter().map(|e| resolve_val(e, ctx)).collect();
                data.insert("data".into(), Value::Array(vals));
            }
        }
    }
    let event = Value::Object(data);
    info!("effect: {event}");
    let _ = state.broadcast.send(event);
}

// ---------------------------------------------------------------------------
// expression evaluation
// ---------------------------------------------------------------------------

fn eval_expr<'a>(
    state: &'a AppState,
    expr: &'a Expr,
    ctx: &'a mut Ctx,
) -> Pin<Box<dyn Future<Output = Result<Value, FlowErr>> + Send + 'a>> {
    Box::pin(async move {
        match expr {
            Expr::Literal(lit) => Ok(lit_json(lit)),
            Expr::DotPath(dp) => Ok(resolve_dot(dp, ctx)),
            Expr::Unary { op, operand } => {
                let v = eval_expr(state, operand, &mut *ctx).await?;
                Ok(eval_unary(op, &v))
            }
            Expr::Binary { op, left, right } => {
                let l = eval_expr(state, left, &mut *ctx).await?;
                let r = eval_expr(state, right, &mut *ctx).await?;
                Ok(eval_binary(op, &l, &r))
            }
            Expr::Ternary { op, a, b, c } => {
                let va = eval_expr(state, a, &mut *ctx).await?;
                let vb = eval_expr(state, b, &mut *ctx).await?;
                let vc = eval_expr(state, c, &mut *ctx).await?;
                Ok(eval_ternary(op, &va, &vb, &vc))
            }
            Expr::If { cond, then, else_ } => {
                let c = eval_expr(state, cond, &mut *ctx).await?;
                if is_truthy(&c) {
                    eval_expr(state, then, &mut *ctx).await
                } else {
                    eval_expr(state, else_, &mut *ctx).await
                }
            }
            Expr::Fetch {
                source,
                filters,
                or_code,
                or_message,
                ..
            } => exec_fetch(state, source, filters, *or_code, or_message.as_deref(), ctx).await,
            Expr::Query {
                source,
                filters,
                sorts,
                page_size,
                ..
            } => exec_query(state, source, filters, sorts, page_size.as_deref(), ctx).await,
            Expr::Call {
                service,
                method,
                args,
                or_code,
                or_message,
            } => exec_service_call(state, service, method, args, *or_code, or_message.as_deref(), ctx).await,
            Expr::WasmCall { .. } => {
                Err(FlowErr::Internal("wasm calls not supported in interpreter".into()))
            }
            Expr::Aggregate { op, source, field } => {
                let arr = eval_expr(state, source, &mut *ctx).await?;
                if let Value::Array(items) = &arr {
                    let values: Vec<&Value> = if let Some(f) = field {
                        items.iter().filter_map(|i| i.get(f.as_str())).collect()
                    } else {
                        items.iter().collect()
                    };
                    Ok(aggregate_values(op, &values))
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::NowOffset {
                direction,
                amount,
                unit,
            } => {
                let amt = eval_expr(state, amount, &mut *ctx).await?;
                let n = val_to_f64(&amt) as i64;
                let now = chrono::Utc::now();
                let delta = match unit {
                    TimeUnit::Seconds => chrono::Duration::seconds(n),
                    TimeUnit::Minutes => chrono::Duration::minutes(n),
                    TimeUnit::Hours => chrono::Duration::hours(n),
                    TimeUnit::Days => chrono::Duration::days(n),
                    TimeUnit::Weeks => chrono::Duration::weeks(n),
                    TimeUnit::Months => chrono::Duration::days(n * 30),
                    TimeUnit::Years => chrono::Duration::days(n * 365),
                };
                let result = match direction {
                    OffsetDirection::Plus => now + delta,
                    OffsetDirection::Minus => now - delta,
                };
                Ok(Value::String(result.to_rfc3339()))
            }
            Expr::Coalesce { value, default } => {
                let v = eval_expr(state, value, &mut *ctx).await?;
                if v.is_null() {
                    eval_expr(state, default, &mut *ctx).await
                } else {
                    Ok(v)
                }
            }
            Expr::Cached { ttl, expr: inner } => {
                let cache_key = format!("{inner:?}");
                {
                    let cache = state.cache.lock().unwrap();
                    if let Some(entry) = cache.get(&cache_key) {
                        if entry.expires > Instant::now() {
                            return Ok(entry.value.clone());
                        }
                    }
                }
                let val = eval_expr(state, inner, &mut *ctx).await?;
                {
                    let mut cache = state.cache.lock().unwrap();
                    cache.insert(
                        cache_key,
                        CacheEntry {
                            value: val.clone(),
                            expires: Instant::now() + StdDuration::from_secs(*ttl as u64),
                        },
                    );
                }
                Ok(val)
            }
            Expr::MapExpr { source, fields } => {
                let arr = eval_expr(state, source, &mut *ctx).await?;
                if let Value::Array(items) = arr {
                    let mapped: Vec<Value> = items
                        .iter()
                        .map(|item| {
                            let mut obj = serde_json::Map::new();
                            for f in fields {
                                if let Some(v) = item.get(f.as_str()) {
                                    obj.insert(f.clone(), v.clone());
                                }
                            }
                            Value::Object(obj)
                        })
                        .collect();
                    Ok(Value::Array(mapped))
                } else {
                    Ok(Value::Array(vec![]))
                }
            }
            Expr::FilterExpr {
                source,
                condition,
            } => {
                let arr = eval_expr(state, source, &mut *ctx).await?;
                if let Value::Array(items) = arr {
                    let mut filtered = Vec::new();
                    for item in items {
                        let mut temp_keys = Vec::new();
                        if let Value::Object(obj) = &item {
                            for (k, v) in obj {
                                if !ctx.bindings.contains_key(k) {
                                    ctx.bindings.insert(k.clone(), v.clone());
                                    temp_keys.push(k.clone());
                                }
                            }
                        }
                        let val = eval_expr(state, condition, &mut *ctx).await?;
                        for k in &temp_keys {
                            ctx.bindings.remove(k);
                        }
                        if is_truthy(&val) {
                            filtered.push(item);
                        }
                    }
                    Ok(Value::Array(filtered))
                } else {
                    Ok(Value::Array(vec![]))
                }
            }
            Expr::ReduceExpr { op, source, field } => {
                let arr = eval_expr(state, source, &mut *ctx).await?;
                if let Value::Array(items) = &arr {
                    let values: Vec<&Value> =
                        items.iter().filter_map(|i| i.get(field.as_str())).collect();
                    Ok(aggregate_values(op, &values))
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::SplitExpr { value, delimiter } => {
                let v = eval_expr(state, value, &mut *ctx).await?;
                let d = eval_expr(state, delimiter, &mut *ctx).await?;
                let parts: Vec<Value> = val_to_string(&v)
                    .split(&val_to_string(&d))
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                Ok(Value::Array(parts))
            }
            Expr::ReplaceExpr { value, from, to } => {
                let v = eval_expr(state, value, &mut *ctx).await?;
                let f = eval_expr(state, from, &mut *ctx).await?;
                let t = eval_expr(state, to, &mut *ctx).await?;
                Ok(Value::String(
                    val_to_string(&v).replace(&val_to_string(&f), &val_to_string(&t)),
                ))
            }
            Expr::FormatExpr { template, args } => {
                let mut result = template.clone();
                for (i, arg) in args.iter().enumerate() {
                    let val = eval_expr(state, arg, &mut *ctx).await?;
                    result = result.replace(&format!("{{{i}}}"), &val_to_string(&val));
                }
                Ok(Value::String(result))
            }
            Expr::Render { template, vars } => {
                let tmpl = state.templates.get(template)
                    .ok_or_else(|| FlowErr::Internal(format!("template not found: {template}")))?;
                let mut rendered = tmpl.clone();
                for (key, val_expr) in vars {
                    let val = eval_expr(state, val_expr, &mut *ctx).await?;
                    rendered = rendered.replace(&format!("{{{{{key}}}}}"), &val_to_string(&val));
                }
                Ok(Value::String(rendered))
            }
            Expr::Translate { key, vars } => {
                let locale = ctx.bindings.get("_locale")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&state.default_locale)
                    .to_string();
                let translations = state.locales.get(&locale)
                    .or_else(|| state.locales.get(&state.default_locale));
                let raw = translations
                    .and_then(|t| {
                        let mut current = t;
                        for segment in key.split('.') {
                            current = current.get(segment)?;
                        }
                        current.as_str()
                    })
                    .unwrap_or(key)
                    .to_string();
                let mut result = raw;
                for (name, val_expr) in vars {
                    let val = eval_expr(state, val_expr, &mut *ctx).await?;
                    result = result.replace(&format!("{{{{{name}}}}}"), &val_to_string(&val));
                }
                Ok(Value::String(result))
            }
            Expr::FuncCall { name, args } => {
                let func_idx = state
                    .funcs
                    .get(name)
                    .ok_or_else(|| FlowErr::Internal(format!("unknown func: {name}")))?;
                let func = match &state.program.constructs[*func_idx] {
                    Construct::Func(f) => f,
                    _ => return Err(FlowErr::Internal("invalid func ref".into())),
                };

                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(eval_expr(state, arg, &mut *ctx).await?);
                }

                let old_bindings = ctx.bindings.clone();
                for (param, val) in func.inputs.iter().zip(arg_vals) {
                    ctx.bindings.insert(param.name.clone(), val);
                }

                exec_steps(state, &func.steps, &mut *ctx).await?;
                let result = eval_expr(state, &func.return_expr, &mut *ctx).await?;

                ctx.bindings = old_bindings;
                Ok(result)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// value helpers
// ---------------------------------------------------------------------------

fn lit_json(lit: &LiteralValue) -> Value {
    match lit {
        LiteralValue::Int(n) => Value::Number((*n).into()),
        LiteralValue::Decimal(s) => {
            s.parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(s.clone()))
        }
        LiteralValue::String(s) => Value::String(s.clone()),
        LiteralValue::Bool(b) => Value::Bool(*b),
        LiteralValue::Ident(s) => Value::String(s.clone()),
        LiteralValue::Now => Value::String("now".into()),
        LiteralValue::None => Value::Null,
    }
}

fn resolve_dot(dp: &DotPath, ctx: &Ctx) -> Value {
    let segs = &dp.segments;
    if segs.is_empty() {
        return Value::Null;
    }
    match segs[0].as_str() {
        "path" if segs.len() == 2 => ctx
            .path
            .get(&segs[1])
            .map(|v| Value::String(v.clone()))
            .unwrap_or(Value::Null),
        "query" if segs.len() == 2 => ctx
            .query
            .get(&segs[1])
            .map(|v| {
                if v == "true" {
                    Value::Bool(true)
                } else if v == "false" {
                    Value::Bool(false)
                } else if let Ok(n) = v.parse::<i64>() {
                    Value::Number(n.into())
                } else {
                    Value::String(v.clone())
                }
            })
            .unwrap_or(Value::Null),
        "body" if segs.len() >= 2 => {
            let mut v = &ctx.body;
            for seg in &segs[1..] {
                v = match v.get(seg) {
                    Some(val) => val,
                    None => return Value::Null,
                };
            }
            v.clone()
        }
        "auth" => {
            let mut v = &ctx.claims;
            for seg in &segs[1..] {
                v = match v.get(seg) {
                    Some(val) => val,
                    None => return Value::Null,
                };
            }
            v.clone()
        }
        other => {
            let mut v = ctx.bindings.get(other).cloned().unwrap_or_else(|| {
                if segs.len() == 1 {
                    Value::String(other.to_string())
                } else {
                    Value::Null
                }
            });
            for seg in &segs[1..] {
                v = v.get(seg).cloned().unwrap_or(Value::Null);
            }
            v
        }
    }
}

fn resolve_val(expr: &Expr, ctx: &Ctx) -> Value {
    match expr {
        Expr::Literal(lit) => lit_json(lit),
        Expr::DotPath(dp) => resolve_dot(dp, ctx),
        _ => Value::Null,
    }
}

fn is_query_ref(expr: &Expr) -> bool {
    matches!(expr, Expr::DotPath(dp) if dp.segments.first().is_some_and(|s| s == "query"))
}

fn is_body_ref(expr: &Expr) -> bool {
    matches!(expr, Expr::DotPath(dp) if dp.segments.first().is_some_and(|s| s == "body"))
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => {
            if let Some(items) = o.get("items") {
                is_truthy(items)
            } else {
                !o.is_empty()
            }
        }
    }
}

fn val_to_f64(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.parse().unwrap_or(0.0),
        Value::Bool(true) => 1.0,
        _ => 0.0,
    }
}

fn val_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

fn val_arith(l: &Value, r: &Value, f: impl Fn(f64, f64) -> f64) -> Value {
    let result = f(val_to_f64(l), val_to_f64(r));
    if result == result.trunc() && result.abs() < i64::MAX as f64 {
        Value::Number((result as i64).into())
    } else {
        serde_json::Number::from_f64(result)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn val_cmp(l: &Value, r: &Value) -> Option<std::cmp::Ordering> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => {
            a.as_f64().unwrap_or(0.0).partial_cmp(&b.as_f64().unwrap_or(0.0))
        }
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => {
            let a = val_to_f64(l);
            let b = val_to_f64(r);
            a.partial_cmp(&b)
        }
    }
}

fn compare_values(left: &Value, op: &CompareOp, right: &Value) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Neq => left != right,
        CompareOp::Gt => val_cmp(left, right) == Some(std::cmp::Ordering::Greater),
        CompareOp::Gte => matches!(
            val_cmp(left, right),
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        ),
        CompareOp::Lt => val_cmp(left, right) == Some(std::cmp::Ordering::Less),
        CompareOp::Lte => matches!(
            val_cmp(left, right),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ),
        CompareOp::In => {
            if let Value::Array(arr) = right {
                arr.contains(left)
            } else {
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// expression evaluation helpers
// ---------------------------------------------------------------------------

fn eval_unary(op: &UnaryOp, v: &Value) -> Value {
    match op {
        UnaryOp::Not => Value::Bool(!is_truthy(v)),
        UnaryOp::Empty => Value::Bool(match v {
            Value::Array(a) => a.is_empty(),
            Value::Object(o) => {
                if let Some(items) = o.get("items") {
                    matches!(items, Value::Array(a) if a.is_empty())
                } else {
                    o.is_empty()
                }
            }
            Value::String(s) => s.is_empty(),
            Value::Null => true,
            _ => false,
        }),
        UnaryOp::Exists => Value::Bool(!v.is_null()),
        UnaryOp::Lower => Value::String(val_to_string(v).to_lowercase()),
        UnaryOp::Upper => Value::String(val_to_string(v).to_uppercase()),
        UnaryOp::Trim => Value::String(val_to_string(v).trim().to_string()),
        UnaryOp::Abs => serde_json::Number::from_f64(val_to_f64(v).abs())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        UnaryOp::Ceil => Value::Number((val_to_f64(v).ceil() as i64).into()),
        UnaryOp::Floor => Value::Number((val_to_f64(v).floor() as i64).into()),
        UnaryOp::Length => match v {
            Value::Array(a) => Value::Number(a.len().into()),
            Value::String(s) => Value::Number(s.len().into()),
            _ => Value::Number(0.into()),
        },
        UnaryOp::First => match v {
            Value::Array(a) => a.first().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        },
        UnaryOp::Last => match v {
            Value::Array(a) => a.last().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        },
        UnaryOp::ToInt => Value::Number((val_to_f64(v) as i64).into()),
        UnaryOp::ToDecimal => serde_json::Number::from_f64(val_to_f64(v))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        UnaryOp::ToString => Value::String(val_to_string(v)),
        UnaryOp::Count => match v {
            Value::Array(a) => Value::Number(a.len().into()),
            _ => Value::Number(0.into()),
        },
    }
}

fn eval_binary(op: &BinaryOp, l: &Value, r: &Value) -> Value {
    match op {
        BinaryOp::Add => val_arith(l, r, |a, b| a + b),
        BinaryOp::Sub => val_arith(l, r, |a, b| a - b),
        BinaryOp::Mul => val_arith(l, r, |a, b| a * b),
        BinaryOp::Div => val_arith(l, r, |a, b| if b != 0.0 { a / b } else { 0.0 }),
        BinaryOp::Mod => val_arith(l, r, |a, b| if b != 0.0 { a % b } else { 0.0 }),
        BinaryOp::And => Value::Bool(is_truthy(l) && is_truthy(r)),
        BinaryOp::Or => Value::Bool(is_truthy(l) || is_truthy(r)),
        BinaryOp::Eq => Value::Bool(l == r),
        BinaryOp::Neq => Value::Bool(l != r),
        BinaryOp::Gt => Value::Bool(val_cmp(l, r) == Some(std::cmp::Ordering::Greater)),
        BinaryOp::Gte => Value::Bool(matches!(
            val_cmp(l, r),
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        )),
        BinaryOp::Lt => Value::Bool(val_cmp(l, r) == Some(std::cmp::Ordering::Less)),
        BinaryOp::Lte => Value::Bool(matches!(
            val_cmp(l, r),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        )),
        BinaryOp::Concat => {
            Value::String(format!("{}{}", val_to_string(l), val_to_string(r)))
        }
        BinaryOp::StartsWith => {
            Value::Bool(val_to_string(l).starts_with(&val_to_string(r)))
        }
        BinaryOp::EndsWith => Value::Bool(val_to_string(l).ends_with(&val_to_string(r))),
        BinaryOp::Contains => match l {
            Value::Array(arr) => Value::Bool(arr.contains(r)),
            Value::String(s) => Value::Bool(s.contains(&val_to_string(r))),
            _ => Value::Bool(false),
        },
        BinaryOp::DaysBetween => {
            if let (Some(a), Some(b)) = (parse_date(l), parse_date(r)) {
                Value::Number(((b - a).num_days()).into())
            } else {
                Value::Number(0.into())
            }
        }
        BinaryOp::HoursBetween => {
            if let (Some(a), Some(b)) = (parse_datetime(l), parse_datetime(r)) {
                Value::Number(((b - a).num_hours()).into())
            } else {
                Value::Number(0.into())
            }
        }
        BinaryOp::MinutesBetween => {
            if let (Some(a), Some(b)) = (parse_datetime(l), parse_datetime(r)) {
                Value::Number(((b - a).num_minutes()).into())
            } else {
                Value::Number(0.into())
            }
        }
        BinaryOp::Round => {
            let n = val_to_f64(l);
            let places = val_to_f64(r) as i32;
            let factor = 10f64.powi(places);
            serde_json::Number::from_f64((n * factor).round() / factor)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        BinaryOp::Coalesce => {
            if l.is_null() {
                r.clone()
            } else {
                l.clone()
            }
        }
        BinaryOp::FormatDate => Value::String(val_to_string(l)),
    }
}

fn eval_ternary(op: &TernaryOp, a: &Value, b: &Value, c: &Value) -> Value {
    match op {
        TernaryOp::Substring => {
            let s = val_to_string(a);
            let start = val_to_f64(b) as usize;
            let len = val_to_f64(c) as usize;
            Value::String(s.chars().skip(start).take(len).collect())
        }
        TernaryOp::Between => Value::Bool(
            matches!(
                val_cmp(a, b),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ) && matches!(
                val_cmp(a, c),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
        ),
    }
}

fn aggregate_values(op: &AggregateOp, values: &[&Value]) -> Value {
    match op {
        AggregateOp::Count => Value::Number(values.len().into()),
        AggregateOp::Sum => {
            let sum: f64 = values.iter().map(|v| val_to_f64(v)).sum();
            if sum == sum.trunc() && sum.abs() < i64::MAX as f64 {
                Value::Number((sum as i64).into())
            } else {
                serde_json::Number::from_f64(sum)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }
        }
        AggregateOp::Avg => {
            if values.is_empty() {
                return Value::Null;
            }
            let sum: f64 = values.iter().map(|v| val_to_f64(v)).sum();
            serde_json::Number::from_f64(sum / values.len() as f64)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        AggregateOp::Min => values
            .iter()
            .copied()
            .min_by(|a, b| val_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or(Value::Null),
        AggregateOp::Max => values
            .iter()
            .copied()
            .max_by(|a, b| val_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or(Value::Null),
        AggregateOp::First => values.first().cloned().cloned().unwrap_or(Value::Null),
        AggregateOp::Last => values.last().cloned().cloned().unwrap_or(Value::Null),
    }
}

fn parse_date(v: &Value) -> Option<chrono::NaiveDate> {
    v.as_str().and_then(|s| s.parse().ok())
}

fn parse_datetime(v: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    v.as_str().and_then(|s| s.parse().ok())
}

// ---------------------------------------------------------------------------
// realm / tenant helpers
// ---------------------------------------------------------------------------

fn find_realm_tenant(program: &Program, realm_name: Option<&str>) -> Option<String> {
    let name = realm_name?;
    for c in &program.constructs {
        if let Construct::Realm(r) = c {
            if r.name == name {
                return r.tenant.clone();
            }
        }
    }
    None
}

fn source_has_field(program: &Program, source_name: &str, field_name: &str) -> bool {
    for c in &program.constructs {
        if let Construct::Source(src) = c {
            if src.name == source_name {
                for sc in &program.constructs {
                    if let Construct::Shape(shape) = sc {
                        if shape.name == src.shape {
                            return shape.fields.iter().any(|f| f.name == field_name);
                        }
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// SERVICE calls (external HTTP or adapter)
// ---------------------------------------------------------------------------

async fn exec_service_call(
    state: &AppState,
    service_name: &str,
    method_name: &str,
    args: &[(String, Expr)],
    or_code: i64,
    or_message: Option<&str>,
    ctx: &mut Ctx,
) -> Result<Value, FlowErr> {
    let svc_idx = state.services.get(service_name).ok_or_else(|| {
        FlowErr::Internal(format!("service not found: {service_name}"))
    })?;
    let svc = match &state.program.constructs[*svc_idx] {
        Construct::Service(s) => s,
        _ => return Err(FlowErr::Internal("service index mismatch".into())),
    };

    let svc_method = svc.methods.iter().find(|m| m.name == method_name).ok_or_else(|| {
        FlowErr::Internal(format!("method {method_name} not found on service {service_name}"))
    })?;

    let mut body = serde_json::Map::new();
    for (name, expr) in args {
        body.insert(name.clone(), resolve_val(expr, ctx));
    }

    let endpoint = resolve_service_endpoint(&svc.endpoint);

    if let Some(adapter_name) = endpoint.strip_prefix("adapter://") {
        return exec_adapter_call(state, adapter_name, method_name, &body, or_code, or_message).await;
    }

    let url = format!("{endpoint}/{method_name}");

    let mut req = state.http_client.post(&url)
        .json(&Value::Object(body));

    if !svc.vault_key.is_empty() {
        if let Ok(key) = std::env::var(&svc.vault_key) {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
    }

    if let Some(timeout) = &svc_method.timeout {
        req = req.timeout(to_std_duration(timeout));
    }

    let max_attempts = svc_method.retry.as_ref().map(|r| r.count as u32 + 1).unwrap_or(1);

    let mut last_err = String::new();
    for attempt in 0..max_attempts {
        if attempt > 0 {
            let delay = match &svc_method.retry {
                Some(r) => match r.strategy {
                    RetryStrategy::Exponential => StdDuration::from_millis(100 * 2u64.pow(attempt - 1)),
                    RetryStrategy::Linear => StdDuration::from_millis(200 * attempt as u64),
                    RetryStrategy::None => StdDuration::from_millis(100),
                },
                None => StdDuration::from_millis(100),
            };
            tokio::time::sleep(delay).await;
        }

        match req.try_clone().unwrap().send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    return resp.json::<Value>().await
                        .map_err(|e| FlowErr::Internal(format!("service response parse: {e}")));
                }
                last_err = format!("service returned {}", resp.status());
                if resp.status().as_u16() < 500 {
                    break;
                }
            }
            Err(e) => {
                last_err = format!("service call failed: {e}");
            }
        }
    }

    service_or_err(or_code, or_message, &last_err)
}

async fn exec_adapter_call(
    state: &AppState,
    adapter_name: &str,
    method_name: &str,
    body: &serde_json::Map<String, Value>,
    or_code: i64,
    or_message: Option<&str>,
) -> Result<Value, FlowErr> {
    let registry = state.adapters.read().await;
    let adapter = registry.get(adapter_name).ok_or_else(|| {
        FlowErr::Internal(format!("adapter not found: {adapter_name}"))
    })?;

    let ep = adapter.endpoints.get(method_name).ok_or_else(|| {
        FlowErr::Internal(format!("adapter {adapter_name} has no method {method_name}"))
    })?;

    if ep.url == "local" {
        // native adapter — try to load from adapters dir
        // Build the input envelope the same way a WASM host would
        let mut input = serde_json::Map::new();
        input.insert("method".into(), Value::String(method_name.into()));
        input.insert("args".into(), Value::Object(body.clone()));
        input.insert("config".into(), {
            let mut cfg = serde_json::Map::new();
            for (k, v) in &adapter.config {
                cfg.insert(k.clone(), Value::String(v.clone()));
            }
            Value::Object(cfg)
        });
        return Ok(Value::Object(input));
    }

    // HTTP adapter endpoint — resolve config vars in URL
    let mut url = ep.url.clone();
    for (k, v) in &adapter.config {
        url = url.replace(&format!("${{{k}}}"), v);
    }
    // Also resolve body field references in URL (e.g., ${receipt_data})
    for (k, v) in body.iter() {
        url = url.replace(&format!("${{{k}}}"), &val_to_string(v));
    }

    let req = state.http_client
        .request(
            match ep.method.as_str() {
                "GET" => reqwest::Method::GET,
                "PUT" => reqwest::Method::PUT,
                "DELETE" => reqwest::Method::DELETE,
                _ => reqwest::Method::POST,
            },
            &url,
        )
        .json(&Value::Object(body.clone()));

    let result = if let Some(secret) = adapter.config.get("secret_key")
        .or_else(|| adapter.config.get("auth_token"))
        .or_else(|| adapter.config.get("server_key"))
    {
        let req = req.bearer_auth(secret);
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<Value>().await
                    .map_err(|e| FlowErr::Internal(format!("adapter response parse: {e}")))
            }
            Ok(resp) => {
                let err = format!("adapter returned {}", resp.status());
                service_or_err(or_code, or_message, &err)
            }
            Err(e) => {
                let err = format!("adapter call failed: {e}");
                service_or_err(or_code, or_message, &err)
            }
        }
    } else {
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<Value>().await
                    .map_err(|e| FlowErr::Internal(format!("adapter response parse: {e}")))
            }
            Ok(resp) => {
                let err = format!("adapter returned {}", resp.status());
                service_or_err(or_code, or_message, &err)
            }
            Err(e) => {
                let err = format!("adapter call failed: {e}");
                service_or_err(or_code, or_message, &err)
            }
        }
    }?;

    if is_request_transform(&result) {
        return exec_request_transform(&state.http_client, &result).await
            .map_err(|e| match e {
                FlowErr::Http(code, msg) => FlowErr::Http(code, msg),
                FlowErr::Internal(msg) => {
                    let or_msg = or_message.unwrap_or(&msg);
                    FlowErr::Http(
                        StatusCode::from_u16(or_code as u16).unwrap_or(StatusCode::BAD_GATEWAY),
                        or_msg.into(),
                    )
                }
            });
    }

    Ok(result)
}

fn service_or_err(or_code: i64, or_message: Option<&str>, last_err: &str) -> Result<Value, FlowErr> {
    if or_code > 0 {
        Err(FlowErr::Http(
            StatusCode::from_u16(or_code as u16).unwrap_or(StatusCode::BAD_GATEWAY),
            or_message.unwrap_or(last_err).into(),
        ))
    } else {
        Err(FlowErr::Internal(last_err.into()))
    }
}

fn is_request_transform(v: &Value) -> bool {
    v.get("_request").is_some()
}

async fn exec_request_transform(
    client: &reqwest::Client,
    envelope: &Value,
) -> Result<Value, FlowErr> {
    let req = envelope.get("_request").ok_or_else(|| {
        FlowErr::Internal("_request field missing from envelope".into())
    })?;

    let url = req.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
        FlowErr::Internal("_request.url is required".into())
    })?;
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("POST");

    let mut http_req = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        _ => client.post(url),
    };

    if let Some(headers) = req.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers {
            if let Some(val) = v.as_str() {
                http_req = http_req.header(k.as_str(), val);
            }
        }
    }

    if let Some(body) = req.get("body") {
        if let Some(body_str) = body.as_str() {
            http_req = http_req.body(body_str.to_string());
        } else {
            http_req = http_req.json(body);
        }
    }

    let resp = http_req.send().await.map_err(|e| {
        FlowErr::Internal(format!("_request HTTP call failed: {e}"))
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(FlowErr::Internal(format!("_request returned {status}: {body}")));
    }

    let resp_json: Value = resp.json().await.map_err(|e| {
        FlowErr::Internal(format!("_request response parse failed: {e}"))
    })?;

    if let Some(transform) = envelope.get("_transform").and_then(|v| v.as_object()) {
        let mut result = serde_json::Map::new();
        for (output_key, source_path) in transform {
            if let Some(path_str) = source_path.as_str() {
                let val = navigate_json_path(&resp_json, path_str);
                result.insert(output_key.clone(), val);
            }
        }
        Ok(Value::Object(result))
    } else {
        Ok(resp_json)
    }
}

fn navigate_json_path(root: &Value, path: &str) -> Value {
    let mut current = root;
    for segment in path.split('.') {
        match current.get(segment) {
            Some(v) => current = v,
            None => return Value::Null,
        }
    }
    current.clone()
}

fn resolve_service_endpoint(endpoint: &str) -> String {
    if let Some(var) = endpoint.strip_prefix('$') {
        std::env::var(var).unwrap_or_else(|_| endpoint.to_string())
    } else if endpoint.starts_with("http") {
        endpoint.to_string()
    } else {
        std::env::var(endpoint).unwrap_or_else(|_| format!("http://{endpoint}"))
    }
}

async fn adapter_watcher(dir: std::path::PathBuf, registry: Arc<tokio::sync::RwLock<AdapterRegistry>>) {
    let mut last_scan: HashMap<String, std::time::SystemTime> = HashMap::new();
    loop {
        tokio::time::sleep(StdDuration::from_secs(2)).await;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let manifest = path.join("adapter.json");
            if !manifest.exists() { continue; }
            let Ok(meta) = manifest.metadata() else { continue };
            let Ok(modified) = meta.modified() else { continue };
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if last_scan.get(&name).map(|&t| modified > t).unwrap_or(true) {
                last_scan.insert(name, modified);
                let mut reg = registry.write().await;
                reg.reload_adapter(&path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Redis adapter dispatch
// ---------------------------------------------------------------------------

#[cfg(feature = "redis-port")]
fn convert_filter_op(op: &FilterOp) -> AdapterFilterOp {
    match op {
        FilterOp::Eq => AdapterFilterOp::Eq,
        FilterOp::Neq => AdapterFilterOp::Neq,
        FilterOp::Gt => AdapterFilterOp::Gt,
        FilterOp::Gte => AdapterFilterOp::Gte,
        FilterOp::Lt => AdapterFilterOp::Lt,
        FilterOp::Lte => AdapterFilterOp::Lte,
        FilterOp::In => AdapterFilterOp::In,
        FilterOp::Like => AdapterFilterOp::Like,
        FilterOp::Contains => AdapterFilterOp::Contains,
        FilterOp::StartsWith => AdapterFilterOp::StartsWith,
        FilterOp::Between => AdapterFilterOp::Between,
    }
}

#[cfg(feature = "redis-port")]
fn compare_op_to_adapter(op: &CompareOp) -> AdapterFilterOp {
    match op {
        CompareOp::Eq => AdapterFilterOp::Eq,
        CompareOp::Neq => AdapterFilterOp::Neq,
        CompareOp::Gt => AdapterFilterOp::Gt,
        CompareOp::Gte => AdapterFilterOp::Gte,
        CompareOp::Lt => AdapterFilterOp::Lt,
        CompareOp::Lte => AdapterFilterOp::Lte,
        CompareOp::In => AdapterFilterOp::In,
    }
}

#[cfg(feature = "redis-port")]
fn to_adapter_filters(filters: &[FilterClause], ctx: &Ctx) -> Vec<FilterParam> {
    filters.iter().map(|f| FilterParam {
        field: f.field.clone(),
        op: convert_filter_op(&f.op),
        value: resolve_val(&f.value, ctx),
    }).collect()
}

#[cfg(feature = "redis-port")]
async fn redis_fetch(
    adapter: &RedisPort,
    source: &str,
    filters: &[FilterClause],
    or_code: i64,
    or_message: Option<&str>,
    ctx: &Ctx,
) -> Result<Value, FlowErr> {
    let adapter_filters = to_adapter_filters(filters, ctx);
    let req = adapter_trait::FetchRequest {
        source: source.into(),
        filters: adapter_filters,
    };

    match adapter.fetch(req).await {
        Ok(Some(val)) => Ok(val),
        Ok(None) if or_code == 0 => Ok(Value::Null),
        Ok(None) => {
            let status = StatusCode::from_u16(or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
            Err(FlowErr::Http(status, or_message.unwrap_or("not found").into()))
        }
        Err(e) => Err(FlowErr::Internal(format!("redis fetch: {e}"))),
    }
}

#[cfg(feature = "redis-port")]
async fn redis_insert(
    adapter: &RedisPort,
    source: &str,
    fields: &[(String, Expr)],
    binding: Option<&String>,
    ctx: &mut Ctx,
) -> Result<(), FlowErr> {
    let mut map = HashMap::new();
    for (field, expr) in fields {
        let val = resolve_val(expr, ctx);
        if !val.is_null() {
            map.insert(field.clone(), val);
        }
    }
    let req = adapter_trait::InsertRequest {
        source: source.into(),
        fields: map,
    };

    let result = adapter.insert(req).await
        .map_err(|e| FlowErr::Internal(format!("redis insert: {e}")))?;
    if let Some(b) = binding {
        ctx.bindings.insert(b.clone(), result);
    }
    Ok(())
}

#[cfg(feature = "redis-port")]
async fn redis_update(
    adapter: &RedisPort,
    source: &str,
    filters: &[WhereClause],
    sets: &[SetClause],
    ctx: &Ctx,
) -> Result<(), FlowErr> {
    let adapter_filters: Vec<FilterParam> = filters.iter().map(|w| FilterParam {
        field: w.field.clone(),
        op: compare_op_to_adapter(&w.op),
        value: resolve_val(&w.value, ctx),
    }).collect();

    let id = adapter_filters.iter().find_map(|f| {
        if (f.field == "id" || f.field == "key") && matches!(f.op, AdapterFilterOp::Eq) {
            f.value.as_str().map(|s| s.to_string())
        } else {
            None
        }
    }).ok_or_else(|| FlowErr::Internal("redis UPDATE requires id or key filter".into()))?;

    let fetch_req = adapter_trait::FetchRequest {
        source: source.into(),
        filters: adapter_filters,
    };
    let existing = adapter.fetch(fetch_req).await
        .map_err(|e| FlowErr::Internal(format!("redis update fetch: {e}")))?
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let mut map: HashMap<String, Value> = match existing {
        Value::Object(m) => m.into_iter().collect(),
        _ => HashMap::new(),
    };
    map.insert("id".into(), Value::String(id));
    for sc in sets {
        map.insert(sc.field.clone(), resolve_val(&sc.value, ctx));
    }

    let ins_req = adapter_trait::InsertRequest {
        source: source.into(),
        fields: map,
    };
    adapter.insert(ins_req).await
        .map_err(|e| FlowErr::Internal(format!("redis update: {e}")))?;
    Ok(())
}

#[cfg(feature = "redis-port")]
async fn redis_delete(
    adapter: &RedisPort,
    source: &str,
    wheres: &[WhereClause],
    or_code: i64,
    or_message: Option<&str>,
    ctx: &Ctx,
) -> Result<(), FlowErr> {
    let adapter_filters: Vec<FilterParam> = wheres.iter().map(|w| FilterParam {
        field: w.field.clone(),
        op: compare_op_to_adapter(&w.op),
        value: resolve_val(&w.value, ctx),
    }).collect();
    let req = adapter_trait::DeleteRequest {
        source: source.into(),
        filters: adapter_filters,
    };

    match adapter.delete(req).await {
        Ok(n) if n > 0 => Ok(()),
        Ok(_) => {
            let status = StatusCode::from_u16(or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
            Err(FlowErr::Http(status, or_message.unwrap_or("not found").into()))
        }
        Err(e) => Err(FlowErr::Internal(format!("redis delete: {e}"))),
    }
}

// ---------------------------------------------------------------------------
// SQL execution
// ---------------------------------------------------------------------------

fn require_db<'a>(state: &'a AppState, source: &str) -> Result<&'a DbBackend, FlowErr> {
    state.dbs.get(source).ok_or_else(|| {
        FlowErr::Internal(format!("no database configured for source '{}' (set DATABASE_URL or {}_DATABASE_URL)", source, source.to_uppercase()))
    })
}

async fn exec_fetch(
    state: &AppState,
    source: &str,
    filters: &[FilterClause],
    or_code: i64,
    or_message: Option<&str>,
    ctx: &Ctx,
) -> Result<Value, FlowErr> {
    #[cfg(feature = "redis-port")]
    if let Some(adapter) = state.redis_sources.get(source) {
        return redis_fetch(adapter, source, filters, or_code, or_message, ctx).await;
    }
    let db = require_db(state, source)?;
    let dialect = db.dialect();
    let mut params: Vec<Value> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();

    if let Some((field, val)) = &ctx.tenant {
        if source_has_field(&state.program, source, field) {
            params.push(val.clone());
            clauses.push(format!("{field} = {}", dialect.ph(params.len())));
        }
    }

    for f in filters {
        let val = resolve_val(&f.value, ctx);
        params.push(val);
        clauses.push(format!("{} {} {}", f.field, filter_op_sql(&f.op), dialect.ph(params.len())));
    }

    let where_part = if clauses.is_empty() {
        "TRUE".into()
    } else {
        clauses.join(" AND ")
    };
    let sql = format!("SELECT * FROM {source} WHERE {where_part} LIMIT 1");

    match db.fetch_optional(&sql, &params).await? {
        Some(row) => Ok(row),
        None if or_code == 0 => Ok(Value::Null),
        None => {
            let status = StatusCode::from_u16(or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
            Err(FlowErr::Http(
                status,
                or_message.unwrap_or("not found").into(),
            ))
        }
    }
}

async fn exec_query(
    state: &AppState,
    source: &str,
    filters: &[FilterClause],
    sorts: &[SortClause],
    page_size: Option<&Expr>,
    ctx: &Ctx,
) -> Result<Value, FlowErr> {
    #[cfg(feature = "redis-port")]
    if state.redis_sources.contains_key(source) {
        return Err(FlowErr::Internal(format!("QUERY not supported for Redis source '{source}' — use FETCH with a key filter")));
    }
    let db = require_db(state, source)?;
    let dialect = db.dialect();
    let mut params: Vec<Value> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();

    if let Some((field, val)) = &ctx.tenant {
        if source_has_field(&state.program, source, field) {
            params.push(val.clone());
            clauses.push(format!("{field} = {}", dialect.ph(params.len())));
        }
    }

    for f in filters {
        let val = resolve_val(&f.value, ctx);
        if is_query_ref(&f.value) && val.is_null() {
            continue;
        }
        params.push(val);
        clauses.push(format!("{} {} {}", f.field, filter_op_sql(&f.op), dialect.ph(params.len())));
    }

    let where_part = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    let sort_part = if sorts.is_empty() {
        String::new()
    } else {
        let s: Vec<String> = sorts
            .iter()
            .map(|s| {
                let d = match s.direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!("{} {d}", s.field)
            })
            .collect();
        format!(" ORDER BY {}", s.join(", "))
    };

    let limit = match page_size {
        Some(Expr::Literal(LiteralValue::Int(n))) => *n,
        Some(Expr::DotPath(dp)) => {
            let v = resolve_dot(dp, ctx);
            v.as_i64().unwrap_or(50)
        }
        _ => 50,
    };
    let page: i64 = ctx
        .query
        .get("page")
        .and_then(|p| p.parse().ok())
        .unwrap_or(1);
    let offset = (page - 1) * limit;

    let count_sql = format!("SELECT COUNT(*) FROM {source}{where_part}");
    let data_sql =
        format!("SELECT * FROM {source}{where_part}{sort_part} LIMIT {limit} OFFSET {offset}");

    let total = db.count(&count_sql, &params).await?;
    let items = db.fetch_all(&data_sql, &params).await?;

    Ok(
        serde_json::json!({ "items": items, "total": total, "page": page, "page_size": limit }),
    )
}

async fn exec_insert(state: &AppState, ins: &InsertStep, ctx: &mut Ctx) -> Result<(), FlowErr> {
    tracing::debug!(target: "axis::database", source = %ins.source, op = "INSERT", "executing");
    let source = &ins.source;
    #[cfg(feature = "redis-port")]
    if let Some(adapter) = state.redis_sources.get(source.as_str()) {
        return redis_insert(adapter, source, &ins.fields, ins.binding.as_ref(), ctx).await;
    }
    let db = require_db(state, source)?;
    let dialect = db.dialect();
    let mut cols: Vec<&str> = Vec::new();
    let mut phs = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    let pk_uuid = if dialect == Dialect::Mysql || dialect == Dialect::Sqlite {
        find_auto_uuid_pk(&state.program, &ins.source)
    } else {
        None
    };
    if let Some(pk_col) = &pk_uuid {
        let uuid_val = uuid::Uuid::new_v4().to_string();
        cols.push(pk_col);
        params.push(Value::String(uuid_val));
        phs.push(dialect.ph(params.len()));
    }

    for (field, expr) in &ins.fields {
        if pk_uuid.as_deref() == Some(field.as_str()) {
            continue;
        }
        let val = resolve_val(expr, ctx);
        if val.is_null() {
            continue;
        }
        cols.push(field);
        params.push(val);
        phs.push(dialect.ph(params.len()));
    }

    let returning = dialect.returning_star();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}){returning}",
        ins.source,
        cols.join(", "),
        phs.join(", "),
    );

    if dialect == Dialect::Mysql {
        db.execute(&sql, &params).await?;
        if let Some(binding) = &ins.binding {
            if let Some(pk_col) = &pk_uuid {
                let pk_val = params[0].clone();
                let select = format!("SELECT * FROM {} WHERE {} = ? LIMIT 1", ins.source, pk_col);
                let row = db.fetch_one(&select, &[pk_val]).await?;
                ctx.bindings.insert(binding.clone(), row);
            } else {
                let select = format!("SELECT * FROM {} ORDER BY ROWID DESC LIMIT 1", ins.source);
                if let Some(row) = db.fetch_optional(&select, &[]).await? {
                    ctx.bindings.insert(binding.clone(), row);
                }
            }
        }
    } else {
        let row = db.fetch_one(&sql, &params).await?;
        if let Some(binding) = &ins.binding {
            ctx.bindings.insert(binding.clone(), row);
        }
    }

    Ok(())
}

async fn exec_update(state: &AppState, upd: &UpdateStep, ctx: &mut Ctx) -> Result<(), FlowErr> {
    tracing::debug!(target: "axis::database", source = %upd.source, op = "UPDATE", "executing");
    let source = &upd.source;
    #[cfg(feature = "redis-port")]
    if let Some(adapter) = state.redis_sources.get(source.as_str()) {
        return redis_update(adapter, source, &upd.wheres, &upd.sets, ctx).await;
    }
    let db = require_db(state, source)?;
    let dialect = db.dialect();
    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let now_fn = if dialect == Dialect::Sqlite { "datetime('now')" } else { "now()" };

    for c in &state.program.constructs {
        if let Construct::Source(src) = c {
            if src.name == upd.source {
                for sc in &state.program.constructs {
                    if let Construct::Shape(shape) = sc {
                        if shape.name == src.shape {
                            for f in &shape.fields {
                                let is_auto =
                                    f.modifiers.iter().any(|m| matches!(m, Modifier::Auto));
                                let is_pk =
                                    f.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
                                if is_auto && matches!(f.ty, TypeExpr::Timestamp) && !is_pk {
                                    sets.push(format!("{} = {now_fn}", f.name));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    for s in &upd.sets {
        let val = resolve_val(&s.value, ctx);
        if is_body_ref(&s.value) && val.is_null() {
            continue;
        }
        params.push(val);
        sets.push(format!("{} = {}", s.field, dialect.ph(params.len())));
    }

    let mut wheres: Vec<String> = Vec::new();
    let mut where_params: Vec<Value> = Vec::new();
    for w in &upd.wheres {
        let val = resolve_val(&w.value, ctx);
        where_params.push(val);
        let n = params.len() + where_params.len();
        wheres.push(format!("{} {} {}", w.field, compare_op_sql(&w.op), dialect.ph(n)));
    }
    params.extend(where_params);

    let returning = dialect.returning_star();
    let sql = format!(
        "UPDATE {} SET {} WHERE {}{returning}",
        upd.source,
        sets.join(", "),
        wheres.join(" AND "),
    );

    if dialect == Dialect::Mysql {
        let affected = db.execute(&sql, &params).await?;
        if affected == 0 {
            let status =
                StatusCode::from_u16(upd.or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
            return Err(FlowErr::Http(
                status,
                upd.or_message.as_deref().unwrap_or("not found").into(),
            ));
        }
        match &upd.binding {
            Some(UpdateBinding::As(name)) => {
                let where_sql: Vec<String> = upd.wheres.iter()
                    .map(|w| format!("{} {} ?", w.field, compare_op_sql(&w.op)))
                    .collect();
                let select = format!("SELECT * FROM {} WHERE {} LIMIT 1", upd.source, where_sql.join(" AND "));
                let where_vals: Vec<Value> = upd.wheres.iter().map(|w| resolve_val(&w.value, ctx)).collect();
                if let Some(row) = db.fetch_optional(&select, &where_vals).await? {
                    ctx.bindings.insert(name.clone(), row);
                }
            }
            Some(UpdateBinding::Count(name)) => {
                ctx.bindings.insert(name.clone(), Value::Number(affected.into()));
            }
            None => {}
        }
    } else {
        match db.fetch_optional(&sql, &params).await? {
            Some(row) => {
                match &upd.binding {
                    Some(UpdateBinding::As(name)) => {
                        ctx.bindings.insert(name.clone(), row);
                    }
                    Some(UpdateBinding::Count(name)) => {
                        ctx.bindings.insert(name.clone(), Value::Number(1.into()));
                    }
                    None => {}
                }
            }
            None => {
                let status =
                    StatusCode::from_u16(upd.or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
                return Err(FlowErr::Http(
                    status,
                    upd.or_message.as_deref().unwrap_or("not found").into(),
                ));
            }
        }
    }
    Ok(())
}

async fn exec_delete(state: &AppState, del: &DeleteStep, ctx: &mut Ctx) -> Result<(), FlowErr> {
    tracing::debug!(target: "axis::database", source = %del.source, op = "DELETE", "executing");
    let source = &del.source;
    #[cfg(feature = "redis-port")]
    if let Some(adapter) = state.redis_sources.get(source.as_str()) {
        return redis_delete(adapter, source, &del.wheres, del.or_code, del.or_message.as_deref(), ctx).await;
    }
    let db = require_db(state, source)?;
    let dialect = db.dialect();
    let mut params: Vec<Value> = Vec::new();
    let mut wheres: Vec<String> = Vec::new();

    for w in &del.wheres {
        let val = resolve_val(&w.value, ctx);
        params.push(val);
        wheres.push(format!("{} {} {}", w.field, compare_op_sql(&w.op), dialect.ph(params.len())));
    }

    let sql = if dialect == Dialect::Mysql {
        format!("DELETE FROM {} WHERE {}", del.source, wheres.join(" AND "))
    } else {
        format!("DELETE FROM {} WHERE {} RETURNING 1", del.source, wheres.join(" AND "))
    };

    if dialect == Dialect::Mysql {
        let affected = db.execute(&sql, &params).await?;
        if affected == 0 {
            let status =
                StatusCode::from_u16(del.or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
            return Err(FlowErr::Http(
                status,
                del.or_message.as_deref().unwrap_or("not found").into(),
            ));
        }
        Ok(())
    } else {
        match db.fetch_optional(&sql, &params).await? {
            Some(_) => Ok(()),
            None => {
                let status =
                    StatusCode::from_u16(del.or_code as u16).unwrap_or(StatusCode::NOT_FOUND);
                Err(FlowErr::Http(
                    status,
                    del.or_message.as_deref().unwrap_or("not found").into(),
                ))
            }
        }
    }
}

async fn exec_upload(state: &AppState, storage_name: &str, file_val: &Value) -> Result<String, FlowErr> {
    let config = state.storages.get(storage_name).ok_or_else(|| {
        FlowErr::Http(StatusCode::INTERNAL_SERVER_ERROR, format!("undefined storage: {storage_name}"))
    })?;

    let file_bytes = match file_val {
        Value::String(s) => s.as_bytes().to_vec(),
        _ => {
            return Err(FlowErr::Http(
                StatusCode::BAD_REQUEST,
                "upload: expected file data".into(),
            ));
        }
    };

    if let Some(max) = config.max_size {
        if file_bytes.len() as i64 > max {
            return Err(FlowErr::Http(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("file exceeds max size of {} bytes", max),
            ));
        }
    }

    let file_id = uuid::Uuid::new_v4().to_string();
    let ext = if !config.types.is_empty() { &config.types[0] } else { "bin" };
    let filename = format!("{file_id}.{ext}");

    let rel_path = match &config.prefix {
        Some(prefix) => format!("{prefix}/{filename}"),
        None => filename.clone(),
    };

    match config.backend {
        StorageBackend::Local => {
            let dir = std::path::Path::new(&config.bucket).join(
                config.prefix.as_deref().unwrap_or(""),
            );
            tokio::fs::create_dir_all(&dir).await.map_err(|e| {
                FlowErr::Http(StatusCode::INTERNAL_SERVER_ERROR, format!("storage mkdir: {e}"))
            })?;
            let full_path = dir.join(&filename);
            tokio::fs::write(&full_path, &file_bytes).await.map_err(|e| {
                FlowErr::Http(StatusCode::INTERNAL_SERVER_ERROR, format!("storage write: {e}"))
            })?;
            tracing::info!(target: "axis::files", storage = storage_name, path = %full_path.display(), "file uploaded");
            if config.access == StorageAccess::Public {
                Ok(format!("/files/{}", rel_path))
            } else {
                Ok(rel_path)
            }
        }
        StorageBackend::S3 => {
            Err(FlowErr::Http(
                StatusCode::NOT_IMPLEMENTED,
                "S3 storage backend not yet implemented".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// SQL helpers
// ---------------------------------------------------------------------------

fn filter_op_sql(op: &FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "=",
        FilterOp::Neq => "!=",
        FilterOp::Gt => ">",
        FilterOp::Gte => ">=",
        FilterOp::Lt => "<",
        FilterOp::Lte => "<=",
        FilterOp::In => "= ANY",
        FilterOp::Like | FilterOp::Contains => "LIKE",
        FilterOp::Between => "BETWEEN",
        FilterOp::StartsWith => "LIKE",
    }
}

fn compare_op_sql(op: &CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "=",
        CompareOp::Neq => "!=",
        CompareOp::Gt => ">",
        CompareOp::Gte => ">=",
        CompareOp::Lt => "<",
        CompareOp::Lte => "<=",
        CompareOp::In => "= ANY",
    }
}

// --- PostgreSQL ---

fn pg_bind<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    v: &Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match v {
        Value::String(s) => {
            if let Ok(u) = uuid::Uuid::parse_str(s) {
                q.bind(u)
            } else {
                q.bind(s.clone())
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn pg_bind_scalar<'q, T>(
    q: sqlx::query::QueryScalar<'q, sqlx::Postgres, T, sqlx::postgres::PgArguments>,
    v: &Value,
) -> sqlx::query::QueryScalar<'q, sqlx::Postgres, T, sqlx::postgres::PgArguments> {
    match v {
        Value::String(s) => {
            if let Ok(u) = uuid::Uuid::parse_str(s) { q.bind(u) }
            else { q.bind(s.clone()) }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn pg_row_json(row: &PgRow) -> Value {
    let mut map = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name();
        let val = match col.type_info().name() {
            "UUID" => row.try_get::<uuid::Uuid, _>(name)
                .map(|u| Value::String(u.to_string())).unwrap_or(Value::Null),
            "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "BPCHAR" => row.try_get::<String, _>(name)
                .map(Value::String).unwrap_or(Value::Null),
            "BOOL" | "BOOLEAN" => row.try_get::<bool, _>(name)
                .map(Value::Bool).unwrap_or(Value::Null),
            "INT2" | "INT4" | "INT8" => row.try_get::<i64, _>(name)
                .map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
            "FLOAT4" | "FLOAT8" => row.try_get::<f64, _>(name).ok()
                .and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null),
            "TIMESTAMPTZ" | "TIMESTAMP" => row.try_get::<chrono::DateTime<chrono::Utc>, _>(name)
                .map(|t| Value::String(t.to_rfc3339())).unwrap_or(Value::Null),
            "DATE" => row.try_get::<chrono::NaiveDate, _>(name)
                .map(|d| Value::String(d.to_string())).unwrap_or(Value::Null),
            "JSONB" | "JSON" => row.try_get::<Value, _>(name).unwrap_or(Value::Null),
            _ => row.try_get::<String, _>(name).map(Value::String).unwrap_or(Value::Null),
        };
        map.insert(name.to_string(), val);
    }
    Value::Object(map)
}

// --- MySQL ---

fn my_bind<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    v: &Value,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match v {
        Value::String(s) => q.bind(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn my_bind_scalar<'q, T>(
    q: sqlx::query::QueryScalar<'q, sqlx::MySql, T, sqlx::mysql::MySqlArguments>,
    v: &Value,
) -> sqlx::query::QueryScalar<'q, sqlx::MySql, T, sqlx::mysql::MySqlArguments> {
    match v {
        Value::String(s) => q.bind(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn my_row_json(row: &MySqlRow) -> Value {
    let mut map = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name();
        let val = match col.type_info().name() {
            "CHAR" | "VARCHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" => row
                .try_get::<String, _>(name).map(Value::String).unwrap_or(Value::Null),
            "BOOLEAN" | "TINYINT(1)" | "BIT" => row
                .try_get::<bool, _>(name).map(Value::Bool).unwrap_or(Value::Null),
            "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" => row
                .try_get::<i64, _>(name).map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
            "FLOAT" | "DOUBLE" | "DECIMAL" => row.try_get::<f64, _>(name).ok()
                .and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null),
            "DATETIME" | "TIMESTAMP" => row.try_get::<chrono::NaiveDateTime, _>(name)
                .map(|t| Value::String(t.to_string())).unwrap_or(Value::Null),
            "DATE" => row.try_get::<chrono::NaiveDate, _>(name)
                .map(|d| Value::String(d.to_string())).unwrap_or(Value::Null),
            "JSON" => row.try_get::<Value, _>(name).unwrap_or(Value::Null),
            _ => row.try_get::<String, _>(name).map(Value::String).unwrap_or(Value::Null),
        };
        map.insert(name.to_string(), val);
    }
    Value::Object(map)
}

// --- SQLite ---

fn sl_bind<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    v: &Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match v {
        Value::String(s) => q.bind(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn sl_bind_scalar<'q, T>(
    q: sqlx::query::QueryScalar<'q, sqlx::Sqlite, T, sqlx::sqlite::SqliteArguments<'q>>,
    v: &Value,
) -> sqlx::query::QueryScalar<'q, sqlx::Sqlite, T, sqlx::sqlite::SqliteArguments<'q>> {
    match v {
        Value::String(s) => q.bind(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(n.to_string()) }
        }
        Value::Bool(b) => q.bind(*b),
        Value::Null => q.bind(Option::<String>::None),
        _ => q.bind(v.to_string()),
    }
}

fn sl_row_json(row: &SqliteRow) -> Value {
    let mut map = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name();
        let val = match col.type_info().name() {
            "TEXT" => row.try_get::<String, _>(name).map(Value::String).unwrap_or(Value::Null),
            "INTEGER" | "INT" | "BIGINT" => row.try_get::<i64, _>(name)
                .map(|n| Value::Number(n.into())).unwrap_or(Value::Null),
            "REAL" | "FLOAT" | "DOUBLE" => row.try_get::<f64, _>(name).ok()
                .and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null),
            "BOOLEAN" => row.try_get::<bool, _>(name).map(Value::Bool).unwrap_or(Value::Null),
            _ => row.try_get::<String, _>(name).map(Value::String).unwrap_or(Value::Null),
        };
        map.insert(name.to_string(), val);
    }
    Value::Object(map)
}

fn find_auto_uuid_pk(program: &Program, source_name: &str) -> Option<String> {
    for c in &program.constructs {
        if let Construct::Source(src) = c {
            if src.name == source_name {
                for sc in &program.constructs {
                    if let Construct::Shape(shape) = sc {
                        if shape.name == src.shape {
                            for f in &shape.fields {
                                let is_pk = f.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
                                let is_auto = f.modifiers.iter().any(|m| matches!(m, Modifier::Auto));
                                if is_pk && is_auto && matches!(f.ty, TypeExpr::Uuid) {
                                    return Some(f.name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// response helpers
// ---------------------------------------------------------------------------

fn return_body_value(ret: &ReturnStmt, ctx: &Ctx) -> Value {
    match &ret.body {
        Some(ReturnBody::Binding(name)) => {
            ctx.bindings.get(name).cloned().unwrap_or(Value::Null)
        }
        Some(ReturnBody::Paginated { .. }) => {
            ctx.bindings.values().next().cloned().unwrap_or(Value::Null)
        }
        Some(ReturnBody::Inline(fields)) => {
            let mut map = serde_json::Map::new();
            for f in fields {
                map.insert(f.name.clone(), resolve_return_value(&f.value, ctx));
            }
            Value::Object(map)
        }
        None => Value::Null,
    }
}

fn resolve_return_value(rv: &ReturnValue, ctx: &Ctx) -> Value {
    match rv {
        ReturnValue::Expr(e) => resolve_val(e, ctx),
        ReturnValue::Nested(fields) => {
            let mut map = serde_json::Map::new();
            for f in fields {
                map.insert(f.name.clone(), resolve_return_value(&f.value, ctx));
            }
            Value::Object(map)
        }
    }
}

fn build_return_resp(ret: &ReturnStmt, ctx: &Ctx) -> Response {
    let status = StatusCode::from_u16(ret.code as u16).unwrap_or(StatusCode::OK);
    let val = return_body_value(ret, ctx);
    if val.is_null() {
        status.into_response()
    } else {
        (status, axum::Json(val)).into_response()
    }
}

fn err_resp(status: StatusCode, msg: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

fn flow_err_resp(e: FlowErr) -> Response {
    match e {
        FlowErr::Http(code, msg) => err_resp(code, &msg),
        FlowErr::Internal(msg) => {
            eprintln!("internal: {msg}");
            err_resp(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
    }
}

// ---------------------------------------------------------------------------
// auto schema creation
// ---------------------------------------------------------------------------

async fn auto_create_tables(program: &Program, dbs: &HashMap<String, DbBackend>) -> Result<(), Box<dyn std::error::Error>> {
    let mut shapes: HashMap<String, &ShapeDef> = HashMap::new();
    let mut sources: Vec<&SourceDef> = Vec::new();
    let mut shape_to_source: HashMap<String, String> = HashMap::new();

    for c in &program.constructs {
        match c {
            Construct::Shape(s) => { shapes.insert(s.name.clone(), s); }
            Construct::Source(s) => {
                shape_to_source.insert(s.shape.clone(), s.name.clone());
                sources.push(s);
            }
            _ => {}
        }
    }

    let mut table_count = 0;
    for source in &sources {
        let db = match dbs.get(&source.name) {
            Some(db) => db,
            None => continue,
        };
        let dialect = db.dialect();

        let shape = match shapes.get(&source.shape) {
            Some(s) => *s,
            None => continue,
        };

        let mut create = format!("CREATE TABLE IF NOT EXISTS {} (\n", source.name);
        for (i, field) in shape.fields.iter().enumerate() {
            let col_type = schema_sql_type(&field.ty, dialect);
            write!(create, "  {} {}", field.name, col_type).unwrap();

            let is_pk = field.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
            let is_required = field.modifiers.iter().any(|m| matches!(m, Modifier::Required));
            let is_unique = field.modifiers.iter().any(|m| matches!(m, Modifier::Unique));
            let is_auto = field.modifiers.iter().any(|m| matches!(m, Modifier::Auto));

            if is_pk { write!(create, " PRIMARY KEY").unwrap(); }
            if is_required && !is_pk { write!(create, " NOT NULL").unwrap(); }
            if is_unique { write!(create, " UNIQUE").unwrap(); }

            if is_auto {
                match (&field.ty, dialect) {
                    (TypeExpr::Uuid, Dialect::Postgres) => write!(create, " DEFAULT gen_random_uuid()").unwrap(),
                    (TypeExpr::Uuid, Dialect::Mysql) => write!(create, " DEFAULT (UUID())").unwrap(),
                    (TypeExpr::Uuid, Dialect::Sqlite) => {}
                    (TypeExpr::Timestamp, Dialect::Sqlite) => write!(create, " DEFAULT (datetime('now'))").unwrap(),
                    (TypeExpr::Timestamp, _) => write!(create, " DEFAULT now()").unwrap(),
                    _ => {}
                }
            }

            for m in &field.modifiers {
                if let Modifier::Default(val) = m {
                    write!(create, " DEFAULT {}", schema_sql_literal(val, dialect)).unwrap();
                }
            }

            if dialect != Dialect::Sqlite {
                if let TypeExpr::Ref { shape: ref_shape, field: ref_field } = &field.ty {
                    let table = shape_to_source.get(ref_shape)
                        .map(|s| s.as_str())
                        .unwrap_or(ref_shape.as_str());
                    write!(create, " REFERENCES {}({})", table.to_lowercase(), ref_field).unwrap();
                }
            }

            if i + 1 < shape.fields.len() {
                writeln!(create, ",").unwrap();
            } else {
                writeln!(create).unwrap();
            }
        }
        create.push(')');

        if let Err(e) = db.execute_ddl(&create).await {
            eprintln!("create table {}: {e}", source.name);
        }

        for field in &shape.fields {
            let col_type = schema_sql_type(&field.ty, dialect);
            let alter = format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
                source.name, field.name, col_type
            );
            let _ = db.execute_ddl(&alter).await;
        }
        table_count += 1;
    }

    if table_count > 0 {
        info!("schema sync: {table_count} tables ensured");
    }
    Ok(())
}

fn schema_sql_type(ty: &TypeExpr, dialect: Dialect) -> String {
    match (ty, dialect) {
        (TypeExpr::Uuid, Dialect::Postgres) => "UUID".into(),
        (TypeExpr::Uuid, Dialect::Mysql) => "CHAR(36)".into(),
        (TypeExpr::Uuid, Dialect::Sqlite) => "TEXT".into(),
        (TypeExpr::String(Some(len)), _) => format!("VARCHAR({len})"),
        (TypeExpr::String(None) | TypeExpr::Text, _) => "TEXT".into(),
        (TypeExpr::Int { .. }, _) => "INTEGER".into(),
        (TypeExpr::Bool, Dialect::Sqlite) => "INTEGER".into(),
        (TypeExpr::Bool, Dialect::Mysql) => "TINYINT(1)".into(),
        (TypeExpr::Bool, Dialect::Postgres) => "BOOLEAN".into(),
        (TypeExpr::Timestamp, Dialect::Postgres) => "TIMESTAMPTZ".into(),
        (TypeExpr::Timestamp, Dialect::Mysql) => "DATETIME".into(),
        (TypeExpr::Timestamp, Dialect::Sqlite) => "TEXT".into(),
        (TypeExpr::Date, Dialect::Sqlite) => "TEXT".into(),
        (TypeExpr::Date, _) => "DATE".into(),
        (TypeExpr::Decimal { precision: _, scale: _ }, Dialect::Sqlite) => "REAL".into(),
        (TypeExpr::Decimal { precision, scale }, _) => {
            let p = precision.unwrap_or(10);
            let s = scale.unwrap_or(2);
            format!("DECIMAL({p},{s})")
        }
        (TypeExpr::Json, Dialect::Postgres) => "JSONB".into(),
        (TypeExpr::Json, Dialect::Mysql) => "JSON".into(),
        (TypeExpr::Json, Dialect::Sqlite) => "TEXT".into(),
        (TypeExpr::Enum(variants), _) => {
            format!("VARCHAR({})", variants.iter().map(|v| v.len()).max().unwrap_or(50))
        }
        (TypeExpr::List(inner), Dialect::Postgres) => format!("{}[]", schema_sql_type(inner, dialect)),
        (TypeExpr::List(_), _) => "JSON".into(),
        (TypeExpr::Maybe(inner), _) => schema_sql_type(inner, dialect),
        (TypeExpr::Map(_, _), Dialect::Postgres) => "JSONB".into(),
        (TypeExpr::Map(_, _), _) => "JSON".into(),
        (TypeExpr::Ref { .. }, Dialect::Postgres) => "UUID".into(),
        (TypeExpr::Ref { .. }, _) => "CHAR(36)".into(),
        (TypeExpr::Blob, Dialect::Postgres) => "BYTEA".into(),
        (TypeExpr::Blob, _) => "BLOB".into(),
    }
}

fn schema_sql_literal(val: &LiteralValue, dialect: Dialect) -> String {
    match val {
        LiteralValue::Int(n) => n.to_string(),
        LiteralValue::Decimal(s) => s.clone(),
        LiteralValue::String(s) => format!("'{}'", s.replace('\'', "''")),
        LiteralValue::Bool(b) => {
            if dialect == Dialect::Sqlite { if *b { "1" } else { "0" }.into() }
            else { b.to_string() }
        }
        LiteralValue::Ident(s) => s.clone(),
        LiteralValue::Now => {
            if dialect == Dialect::Sqlite { "datetime('now')".into() }
            else { "now()".into() }
        }
        LiteralValue::None => "NULL".into(),
    }
}

fn load_templates(tmpl_dir: &Path) -> HashMap<String, String> {
    let mut templates = HashMap::new();
    if !tmpl_dir.exists() {
        return templates;
    }
    load_templates_recursive(tmpl_dir, tmpl_dir, &mut templates);
    if !templates.is_empty() {
        info!("loaded {} templates from {}", templates.len(), tmpl_dir.display());
    }
    templates
}

fn load_templates_recursive(base: &Path, dir: &Path, templates: &mut HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_templates_recursive(base, &path, templates);
        } else if let Ok(content) = std::fs::read_to_string(&path) {
            let key = path.strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            templates.insert(key, content);
        }
    }
}

fn load_locales(locale_dir: &Path) -> (HashMap<String, Value>, String) {
    let mut locales = HashMap::new();
    if !locale_dir.exists() {
        return (locales, "en".into());
    }
    let Ok(entries) = std::fs::read_dir(locale_dir) else {
        return (locales, "en".into());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<Value>(&data) {
                    let name = path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    locales.insert(name, json);
                }
            }
        }
    }
    let default = std::env::var("DEFAULT_LOCALE").unwrap_or_else(|_| "en".into());
    if !locales.is_empty() {
        info!("loaded {} locales (default: {})", locales.len(), default);
    }
    (locales, default)
}
