use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::sql as codegen;
use crate::ast::*;

thread_local! {
    static FLOW_SCOPE: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
    static SOURCE_SHAPES: std::cell::RefCell<HashMap<String, Vec<(String, TypeExpr)>>> = std::cell::RefCell::new(HashMap::new());
    static SOURCE_TYPES: std::cell::RefCell<HashMap<String, SourceType>> = std::cell::RefCell::new(HashMap::new());
    static SOURCE_AUTO_UPDATED: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
    static STORAGES: std::cell::RefCell<HashMap<String, StorageDef>> = std::cell::RefCell::new(HashMap::new());
    static BODY_FIELDS: std::cell::RefCell<HashMap<String, TypeExpr>> = std::cell::RefCell::new(HashMap::new());
    static QUERY_PARAMS: std::cell::RefCell<HashMap<String, TypeExpr>> = std::cell::RefCell::new(HashMap::new());
    static DB_REF_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static DB_DIALECT_OVERRIDE: std::cell::RefCell<Option<Dialect>> = const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    fn now_expr(&self) -> &'static str {
        match self {
            Dialect::Postgres | Dialect::Mysql => "NOW()",
            Dialect::Sqlite => "datetime('now')",
        }
    }
}

fn source_dialect(source: &str) -> Dialect {
    SOURCE_TYPES.with(|st| match st.borrow().get(source) {
        Some(SourceType::Mysql) => Dialect::Mysql,
        Some(SourceType::Sqlite) => Dialect::Sqlite,
        _ => Dialect::Postgres,
    })
}

fn all_used_dialects() -> HashSet<Dialect> {
    SOURCE_TYPES.with(|st| {
        let map = st.borrow();
        if map.is_empty() {
            let mut s = HashSet::new();
            s.insert(Dialect::Postgres);
            return s;
        }
        map.values()
            .map(|t| match t {
                SourceType::Mysql => Dialect::Mysql,
                SourceType::Sqlite => Dialect::Sqlite,
                _ => Dialect::Postgres,
            })
            .collect()
    })
}

fn all_sql_sources() -> Vec<(String, Dialect)> {
    SOURCE_TYPES.with(|st| {
        let mut sources: Vec<(String, Dialect)> = st
            .borrow()
            .iter()
            .filter_map(|(name, t)| match t {
                SourceType::Redis | SourceType::Elasticsearch | SourceType::Dynamodb => None,
                SourceType::Postgres => Some((name.clone(), Dialect::Postgres)),
                SourceType::Mysql => Some((name.clone(), Dialect::Mysql)),
                SourceType::Sqlite => Some((name.clone(), Dialect::Sqlite)),
            })
            .collect();
        sources.sort_by(|left, right| left.0.cmp(&right.0));
        sources
    })
}

fn upload_storages(flows: &[&FlowDef]) -> Vec<StorageDef> {
    fn collect(steps: &[FlowStep], names: &mut HashSet<String>) {
        for step in steps {
            match step {
                FlowStep::Upload(upload) => {
                    names.insert(upload.storage.clone());
                }
                FlowStep::Match(match_step) => {
                    for branch in &match_step.branches {
                        collect(&branch.steps, names);
                    }
                    if let Some(default) = &match_step.default {
                        collect(default, names);
                    }
                }
                FlowStep::Each(each) => collect(&each.steps, names),
                FlowStep::Try(try_step) => {
                    collect(&try_step.body, names);
                    collect(&try_step.recover, names);
                }
                _ => {}
            }
        }
    }

    let mut names = HashSet::new();
    for flow in flows {
        collect(&flow.steps, &mut names);
    }
    let mut storages = STORAGES.with(|definitions| {
        let definitions = definitions.borrow();
        names
            .into_iter()
            .filter_map(|name| definitions.get(&name).cloned())
            .collect::<Vec<_>>()
    });
    storages.sort_by(|left, right| left.name.cmp(&right.name));
    storages
}

fn collect_flow_variables(steps: &[FlowStep]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for step in steps {
        match step {
            FlowStep::Let(l) => {
                vars.insert(l.name.clone());
            }
            FlowStep::Set(s) => {
                vars.insert(s.name.clone());
            }
            FlowStep::Insert(ins) => {
                if let Some(b) = &ins.binding {
                    vars.insert(b.clone());
                }
            }
            FlowStep::Upsert(upsert) => {
                if let Some(b) = &upsert.binding {
                    vars.insert(b.clone());
                }
            }
            FlowStep::Update(upd) => match &upd.binding {
                Some(UpdateBinding::As(n)) | Some(UpdateBinding::Count(n)) => {
                    vars.insert(n.clone());
                }
                None => {}
            },
            FlowStep::Match(m) => {
                for b in &m.branches {
                    vars.extend(collect_flow_variables(&b.steps));
                }
                if let Some(d) = &m.default {
                    vars.extend(collect_flow_variables(d));
                }
            }
            FlowStep::Each(e) => {
                vars.insert(e.binding.clone());
                vars.extend(collect_flow_variables(&e.steps));
            }
            FlowStep::Fanout(_) => {}
            FlowStep::Upload(upload) => {
                vars.insert(upload.binding.clone());
            }
            FlowStep::Try(t) => {
                vars.extend(collect_flow_variables(&t.body));
                vars.extend(collect_flow_variables(&t.recover));
            }
            _ => {}
        }
    }
    vars
}

fn steps_have_upload(steps: &[FlowStep]) -> bool {
    steps.iter().any(|step| match step {
        FlowStep::Upload(_) => true,
        FlowStep::Match(branches) => {
            branches
                .branches
                .iter()
                .any(|branch| steps_have_upload(&branch.steps))
                || branches.default.as_deref().is_some_and(steps_have_upload)
        }
        FlowStep::Each(each) => steps_have_upload(&each.steps),
        FlowStep::Try(try_step) => {
            steps_have_upload(&try_step.body) || steps_have_upload(&try_step.recover)
        }
        _ => false,
    })
}

#[derive(Debug)]
pub struct RustProject {
    pub cargo_toml: String,
    pub main_rs: String,
    pub sql_schema: String,
}

pub fn generate(program: &Program) -> RustProject {
    let codegen_result = codegen::generate(program);
    let sql_schema = codegen_result.sql.clone();

    let flows: Vec<&FlowDef> = program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Flow(f) = c {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    let sagas: Vec<&SagaDef> = program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Saga(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    let surfaces: Vec<&SurfaceDef> = program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Surface(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    let streams: Vec<&StreamDef> = program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Stream(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    let shapes: HashMap<String, &ShapeDef> = program
        .constructs
        .iter()
        .filter_map(|c| {
            if let Construct::Shape(s) = c {
                Some((s.name.clone(), s))
            } else {
                None
            }
        })
        .collect();
    SOURCE_SHAPES.with(|ss| {
        let mut map = ss.borrow_mut();
        map.clear();
        for c in &program.constructs {
            if let Construct::Source(src) = c {
                if let Some(shape) = shapes.get(&src.shape) {
                    let fields: Vec<(String, TypeExpr)> = shape
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect();
                    map.insert(src.name.clone(), fields);
                }
            }
        }
    });
    SOURCE_AUTO_UPDATED.with(|sources| {
        let mut auto_updated = sources.borrow_mut();
        auto_updated.clear();
        for construct in &program.constructs {
            if let Construct::Source(source) = construct {
                if shapes.get(&source.shape).is_some_and(|shape| {
                    shape.fields.iter().any(|field| {
                        field.name == "updated_at"
                            && field
                                .modifiers
                                .iter()
                                .any(|modifier| matches!(modifier, Modifier::Auto))
                    })
                }) {
                    auto_updated.insert(source.name.clone());
                }
            }
        }
    });
    SOURCE_TYPES.with(|st| {
        let mut map = st.borrow_mut();
        map.clear();
        for c in &program.constructs {
            if let Construct::Source(src) = c {
                map.insert(src.name.clone(), src.source_type);
            }
        }
    });
    STORAGES.with(|storages| {
        let mut map = storages.borrow_mut();
        map.clear();
        for construct in &program.constructs {
            if let Construct::Storage(storage) = construct {
                map.insert(storage.name.clone(), storage.clone());
            }
        }
    });

    let has_streams = !streams.is_empty();
    let has_idempotency = flows.iter().any(|flow| flow.idempotency.is_some());
    let has_multipart = flows.iter().any(|flow| {
        flow.body
            .as_ref()
            .is_some_and(|body| body.kind == BodyKind::Multipart)
    });
    let has_upload = flows.iter().any(|flow| steps_have_upload(&flow.steps));
    let has_s3 = upload_storages(&flows)
        .iter()
        .any(|storage| storage.backend == StorageBackend::S3);
    let auth_usage = collect_auth_usage(&flows, &streams);
    let cargo_toml = generate_cargo_toml(
        has_streams,
        has_idempotency,
        has_multipart,
        has_upload,
        has_s3,
        &auth_usage,
    );
    let main_rs = generate_main(&flows, &sagas, &surfaces, &streams, &codegen_result);

    RustProject {
        cargo_toml,
        main_rs,
        sql_schema,
    }
}

fn generate_cargo_toml(
    has_streams: bool,
    has_idempotency: bool,
    has_multipart: bool,
    has_upload: bool,
    has_s3: bool,
    auth: &AuthUsage,
) -> String {
    let mut axum_features = Vec::new();
    if has_streams {
        axum_features.push("ws");
    }
    if has_multipart {
        axum_features.push("multipart");
    }
    let axum_dep = if axum_features.is_empty() {
        r#"axum = "0.8""#.to_string()
    } else {
        format!(
            "axum = {{ version = \"0.8\", features = [{}] }}",
            axum_features
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let dialects = all_used_dialects();
    let mut sqlx_features = vec!["runtime-tokio-rustls", "uuid", "chrono", "rust_decimal"];
    if dialects.contains(&Dialect::Postgres) {
        sqlx_features.push("postgres");
    }
    if dialects.contains(&Dialect::Mysql) {
        sqlx_features.push("mysql");
    }
    if dialects.contains(&Dialect::Sqlite) {
        sqlx_features.push("sqlite");
    }
    let sqlx_feat_str = sqlx_features
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut toml = format!(
        r#"[package]
name = "axis-server"
version = "0.1.0"
edition = "2024"

[dependencies]
{axum_dep}
tokio = {{ version = "1", features = ["full"] }}
sqlx = {{ version = "0.8", features = [{sqlx_feat_str}] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
uuid = {{ version = "1", features = ["v4", "v7", "serde"] }}
chrono = {{ version = "0.4", features = ["serde"] }}
rust_decimal = {{ version = "1", features = ["serde-with-str"] }}
tower-http = {{ version = "0.6", features = ["cors", "trace", "limit", "compression-full", "fs"] }}
tower = {{ version = "0.5", features = ["limit", "timeout"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["json"] }}
prometheus = "0.13"
axum-server = {{ version = "0.7", features = ["tls-rustls"] }}
"#
    );
    if auth.needs_claims {
        toml.push_str("jsonwebtoken = \"9\"\n");
    }
    if auth.api_key || auth.webhook {
        toml.push_str("hmac = \"0.12\"\n");
    }
    if auth.api_key || auth.webhook || has_idempotency {
        toml.push_str("sha2 = \"0.10\"\n");
    }
    if auth.webhook || has_idempotency {
        toml.push_str("hex = \"0.4\"\n");
    }
    if has_streams {
        toml.push_str("axum-extra = { version = \"0.10\", features = [\"typed-header\"] }\n");
        toml.push_str("tokio-stream = \"0.1\"\n");
        toml.push_str("futures = \"0.3\"\n");
    }
    if has_s3 {
        toml.push_str("object_store = { version = \"0.14\", default-features = false, features = [\"aws\"] }\n");
    }
    if has_upload {
        toml.push_str("infer = \"0.19\"\n");
    }
    toml
}

fn generate_main(
    flows: &[&FlowDef],
    _sagas: &[&SagaDef],
    surfaces: &[&SurfaceDef],
    streams: &[&StreamDef],
    codegen_result: &codegen::CodegenResult,
) -> String {
    let mut out = String::new();
    let auth_usage = collect_auth_usage(flows, streams);
    let has_any_rate_limit = flows.iter().any(|f| !f.limits.is_empty());
    let has_any_cache = flows.iter().any(|f| !f.cache.is_empty());
    let has_any_auth = flows
        .iter()
        .any(|f| f.auth.is_some() && !matches!(f.auth, Some(AuthDecl::None)))
        || streams
            .iter()
            .any(|s| s.auth.is_some() && !matches!(s.auth, Some(AuthDecl::None)));
    let has_idempotency = flows.iter().any(|flow| flow.idempotency.is_some());
    let storages = upload_storages(flows);
    let has_multipart = flows.iter().any(|flow| {
        flow.body
            .as_ref()
            .is_some_and(|body| body.kind == BodyKind::Multipart)
    });

    let has_path_params = flows.iter().any(|flow| flow.path.contains(':'));
    let has_query_params = flows.iter().any(|flow| {
        !collect_query_refs(flow).is_empty()
            || flow.steps.iter().any(|step| {
                matches!(
                    step,
                    FlowStep::Let(LetStep {
                        expr: Expr::Query {
                            page_size: Some(_),
                            ..
                        },
                        ..
                    })
                )
            })
    });
    let mut extractors = vec!["State"];
    if has_path_params {
        extractors.push("Path");
    }
    if has_query_params {
        extractors.push("Query");
    }
    if has_multipart {
        extractors.push("FromRequest");
        extractors.push("Multipart");
    }
    writeln!(out, "use axum::{{Router, Json, extract::{{{}}}, http::StatusCode, response::IntoResponse, middleware}};", extractors.join(", ")).unwrap();
    if has_any_auth || has_idempotency {
        writeln!(out, "use axum::http::{{HeaderMap, Request}};").unwrap();
    } else {
        writeln!(out, "use axum::http::Request;").unwrap();
    }
    if auth_usage.needs_claims || has_idempotency {
        writeln!(out, "use serde::{{Deserialize, Serialize}};").unwrap();
    } else {
        writeln!(out, "use serde::Deserialize;").unwrap();
    }
    writeln!(out, "use std::sync::Arc;").unwrap();
    writeln!(
        out,
        "use prometheus::{{IntCounterVec, HistogramVec, Encoder, TextEncoder}};"
    )
    .unwrap();
    if auth_usage.needs_claims {
        writeln!(
            out,
            "use jsonwebtoken::{{decode, DecodingKey, Validation, Algorithm}};"
        )
        .unwrap();
    }
    if has_idempotency {
        writeln!(out, "use sha2::Digest as _;").unwrap();
    }
    writeln!(out, "use tracing::Instrument;").unwrap();
    writeln!(out).unwrap();

    let sources = all_sql_sources();

    writeln!(out, "#[derive(Clone)]").unwrap();
    writeln!(out, "struct AppState {{").unwrap();
    for (src_name, dialect) in &sources {
        let pool_ty = match dialect {
            Dialect::Postgres => "sqlx::PgPool",
            Dialect::Mysql => "sqlx::MySqlPool",
            Dialect::Sqlite => "sqlx::SqlitePool",
        };
        writeln!(out, "    db_{src_name}: {pool_ty},").unwrap();
    }
    for storage in &storages {
        if storage.backend == StorageBackend::S3 {
            writeln!(
                out,
                "    storage_{}: Arc<object_store::aws::AmazonS3>,",
                storage.name
            )
            .unwrap();
        }
    }
    if auth_usage.needs_claims {
        writeln!(out, "    jwt_secret: String,").unwrap();
    }
    if auth_usage.api_key {
        writeln!(out, "    api_key: Option<String>,").unwrap();
    }
    writeln!(out, "    request_counter: IntCounterVec,").unwrap();
    writeln!(out, "    request_duration: HistogramVec,").unwrap();
    if has_any_rate_limit {
        writeln!(out, "    rate_limiters: Arc<std::sync::Mutex<std::collections::HashMap<String, (u64, std::time::Instant)>>>,").unwrap();
    }
    if has_any_cache {
        writeln!(out, "    response_cache: Arc<std::sync::Mutex<std::collections::HashMap<String, (serde_json::Value, u16, std::time::Instant)>>>,").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    generate_auth_module(&mut out, &auth_usage);
    writeln!(out).unwrap();

    generate_api_error(&mut out);
    writeln!(out).unwrap();

    if has_idempotency {
        generate_idempotency_helpers(&mut out);
        writeln!(out).unwrap();
    }
    if !storages.is_empty() {
        generate_upload_helpers(
            &mut out,
            storages.iter().any(|storage| {
                storage.backend == StorageBackend::S3 && storage.access == StorageAccess::Public
            }),
        );
        writeln!(out).unwrap();
    }

    generate_request_id_middleware(&mut out);
    writeln!(out).unwrap();

    if !sources.is_empty() {
        generate_db_try_macro(&mut out);
        writeln!(out).unwrap();
    }

    let dialects = all_used_dialects();
    if !sources.is_empty() {
        generate_row_json_fns(&mut out, &dialects);
    }
    generate_to_decimal(&mut out);

    // Generate structs for shapes
    for route_info in &codegen_result.routes {
        generate_flow_handler(&mut out, route_info, flows);
    }

    // Health check handlers
    writeln!(out, "async fn healthz() -> StatusCode {{").unwrap();
    writeln!(out, "    StatusCode::OK").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    let ready_state = if sources.is_empty() {
        "_state"
    } else {
        "state"
    };
    writeln!(
        out,
        "async fn readyz(State({ready_state}): State<Arc<AppState>>) -> StatusCode {{"
    )
    .unwrap();
    if !sources.is_empty() {
        let checks = sources
            .iter()
            .map(|(source, _)| {
                format!("sqlx::query(\"SELECT 1\").fetch_one(&state.db_{source}).await.is_ok()")
            })
            .collect::<Vec<_>>()
            .join(" && ");
        writeln!(out, "    let db_ok = {checks};").unwrap();
        writeln!(
            out,
            "    if db_ok {{ StatusCode::OK }} else {{ StatusCode::SERVICE_UNAVAILABLE }}"
        )
        .unwrap();
    } else {
        writeln!(out, "    StatusCode::OK").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Prometheus metrics handler
    writeln!(
        out,
        "async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {{"
    )
    .unwrap();
    writeln!(out, "    let _ = &state.request_counter;").unwrap();
    writeln!(out, "    let encoder = TextEncoder::new();").unwrap();
    writeln!(out, "    let metric_families = prometheus::gather();").unwrap();
    writeln!(out, "    let mut buffer = Vec::new();").unwrap();
    writeln!(
        out,
        "    encoder.encode(&metric_families, &mut buffer).unwrap();"
    )
    .unwrap();
    writeln!(
        out,
        "    (StatusCode::OK, [(\"content-type\", \"text/plain\")], buffer)"
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Stream handlers
    for stream in streams {
        generate_stream_handler(&mut out, stream);
    }

    // Router setup
    writeln!(out, "fn build_router(state: Arc<AppState>) -> Router {{").unwrap();
    writeln!(out, "    let mut router = Router::new()").unwrap();
    writeln!(
        out,
        "        .route(\"/healthz\", axum::routing::get(healthz))"
    )
    .unwrap();
    writeln!(
        out,
        "        .route(\"/readyz\", axum::routing::get(readyz))"
    )
    .unwrap();
    writeln!(
        out,
        "        .route(\"/metrics\", axum::routing::get(metrics));"
    )
    .unwrap();
    writeln!(out).unwrap();

    // Add surface routes if any
    if !surfaces.is_empty() {
        for surface in surfaces {
            let base = surface.base_path.as_deref().unwrap_or("");
            for route in &surface.routes {
                let full_path = axum_path(&format!("{}{}", base, route.path));
                let method = match route.method {
                    HttpMethod::Get => "get",
                    HttpMethod::Post => "post",
                    HttpMethod::Put => "put",
                    HttpMethod::Patch => "patch",
                    HttpMethod::Delete => "delete",
                    HttpMethod::Webhook => "post",
                };
                let handler = format!("handle_{}", route.target);
                writeln!(out, "    router = router.route(\"{full_path}\", axum::routing::{method}({handler}));").unwrap();
            }
        }
    } else {
        for route_info in &codegen_result.routes {
            let path = axum_path(&route_info.path);
            let method = axum_method(&route_info.method);
            let handler = format!("handle_{}", route_info.name);
            let flow = flows.iter().find(|f| f.name == route_info.name);
            let body_limit = flow
                .and_then(|flow| flow.body.as_ref())
                .filter(|body| body.kind == BodyKind::Json)
                .map(compute_body_limit);
            let timeout_secs = flow.and_then(|f| f.timeout.as_ref()).map(duration_to_secs);
            let mut layers = Vec::new();
            if let Some(limit) = body_limit {
                layers.push(format!(
                    "tower_http::limit::RequestBodyLimitLayer::new({limit})"
                ));
            }
            if layers.is_empty() && timeout_secs.is_none() {
                writeln!(
                    out,
                    "    router = router.route(\"{path}\", axum::routing::{method}({handler}));"
                )
                .unwrap();
            } else {
                let mut chain = format!("axum::routing::{method}({handler})");
                for layer in &layers {
                    chain = format!("{chain}.layer({layer})");
                }
                if let Some(secs) = timeout_secs {
                    chain = format!(
                        "{chain}.layer(tower::ServiceBuilder::new().layer(axum::error_handling::HandleErrorLayer::new(|_: tower::BoxError| async {{ StatusCode::REQUEST_TIMEOUT }})).timeout(std::time::Duration::from_secs({secs})))"
                    );
                }
                writeln!(out, "    router = router.route(\"{path}\", {chain});").unwrap();
            }
        }
    }

    for stream in streams {
        let path = axum_path(&stream.path);
        let handler = format!("handle_stream_{}", stream.name);
        writeln!(
            out,
            "    router = router.route(\"{path}\", axum::routing::get({handler}));"
        )
        .unwrap();
    }
    for storage in &storages {
        if storage.backend == StorageBackend::Local && storage.access == StorageAccess::Public {
            let bucket =
                serde_json::to_string(&storage.bucket).expect("storage bucket is serializable");
            writeln!(
                out,
                "    router = router.nest_service(\"/files/{}\", tower_http::services::ServeDir::new({bucket}));",
                storage.name
            )
            .unwrap();
        }
    }

    writeln!(out).unwrap();
    writeln!(out, "    router").unwrap();
    writeln!(
        out,
        "        .layer(tower_http::compression::CompressionLayer::new())"
    )
    .unwrap();
    writeln!(
        out,
        "        .layer(middleware::from_fn(request_id_middleware))"
    )
    .unwrap();
    writeln!(
        out,
        "        .layer(tower_http::trace::TraceLayer::new_for_http())"
    )
    .unwrap();
    writeln!(out, "        .layer(build_cors())").unwrap();
    let upload_limit = storages
        .iter()
        .map(|storage| {
            storage
                .max_size
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(64 * 1024 * 1024)
        })
        .max()
        .unwrap_or(9 * 1024 * 1024)
        .saturating_add(1024 * 1024);
    writeln!(
        out,
        "        .layer(tower_http::limit::RequestBodyLimitLayer::new({upload_limit}))"
    )
    .unwrap();
    writeln!(out, "        .layer(tower::ServiceBuilder::new().layer(axum::error_handling::HandleErrorLayer::new(|_: tower::BoxError| async {{ StatusCode::REQUEST_TIMEOUT }})).timeout(std::time::Duration::from_secs(std::env::var(\"REQUEST_TIMEOUT_SECS\").ok().and_then(|v| v.parse().ok()).unwrap_or(30u64))))").unwrap();
    writeln!(out, "        .with_state(state)").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    generate_cors_builder(&mut out);
    writeln!(out).unwrap();

    // Main function
    writeln!(out, "#[tokio::main]").unwrap();
    writeln!(out, "async fn main() {{").unwrap();
    writeln!(out, "    tracing_subscriber::fmt().json().init();").unwrap();
    writeln!(out).unwrap();
    let sources = all_sql_sources();
    for (src_name, dialect) in &sources {
        let env_key = format!("{}_DATABASE_URL", src_name.to_uppercase());
        writeln!(out, "    let {src_name}_url = std::env::var(\"{env_key}\")").unwrap();
        writeln!(out, "        .or_else(|_| std::env::var(\"DATABASE_URL\"))").unwrap();
        writeln!(
            out,
            "        .expect(\"{env_key} or DATABASE_URL must be set\");"
        )
        .unwrap();
        match dialect {
            Dialect::Postgres => {
                writeln!(
                    out,
                    "    let db_{src_name} = sqlx::postgres::PgPoolOptions::new()"
                )
                .unwrap();
                writeln!(out, "        .max_connections(10)").unwrap();
                writeln!(out, "        .connect(&{src_name}_url).await").unwrap();
                writeln!(out, "        .expect(\"failed to connect {src_name}\");").unwrap();
            }
            Dialect::Mysql => {
                writeln!(
                    out,
                    "    let db_{src_name} = sqlx::mysql::MySqlPoolOptions::new()"
                )
                .unwrap();
                writeln!(out, "        .max_connections(10)").unwrap();
                writeln!(out, "        .connect(&{src_name}_url).await").unwrap();
                writeln!(out, "        .expect(\"failed to connect {src_name}\");").unwrap();
            }
            Dialect::Sqlite => {
                writeln!(
                    out,
                    "    let db_{src_name} = sqlx::sqlite::SqlitePoolOptions::new()"
                )
                .unwrap();
                writeln!(out, "        .max_connections(if {src_name}_url == \"sqlite::memory:\" || {src_name}_url == \"sqlite://:memory:\" || {src_name}_url.ends_with(\"mode=memory\") {{ 1 }} else {{ 5 }})").unwrap();
                writeln!(out, "        .connect(&{src_name}_url).await").unwrap();
                writeln!(out, "        .expect(\"failed to connect {src_name}\");").unwrap();
            }
        }
        writeln!(out).unwrap();
    }
    for storage in &storages {
        if storage.backend == StorageBackend::S3 {
            let bucket =
                serde_json::to_string(&storage.bucket).expect("storage bucket is serializable");
            writeln!(out, "    let storage_{} = Arc::new(object_store::aws::AmazonS3Builder::from_env().with_bucket_name({bucket}).build().expect(\"invalid S3 storage configuration\"));", storage.name).unwrap();
        }
    }
    if storages
        .iter()
        .any(|storage| storage.backend == StorageBackend::S3)
    {
        writeln!(out).unwrap();
    }
    for flow in flows.iter().filter(|flow| flow.idempotency.is_some()) {
        let mut flow_sources = HashSet::new();
        collect_all_sources(&flow.steps, &mut flow_sources);
        let mut flow_sources: Vec<String> = flow_sources.into_iter().collect();
        flow_sources.sort();
        if let Some(first) = flow_sources.first() {
            for other in flow_sources.iter().skip(1) {
                writeln!(out, "    assert_eq!({first}_url, {other}_url, \"idempotent flow '{}' requires every SQL source to use the same database URL\");", flow.name).unwrap();
            }
        }
    }
    writeln!(out, "    let request_counter = IntCounterVec::new(").unwrap();
    writeln!(
        out,
        "        prometheus::opts!(\"axis_request_total\", \"Total HTTP requests\"),"
    )
    .unwrap();
    writeln!(out, "        &[\"flow\", \"method\", \"status\"],").unwrap();
    writeln!(out, "    ).unwrap();").unwrap();
    writeln!(out, "    let request_duration = HistogramVec::new(").unwrap();
    writeln!(out, "        prometheus::histogram_opts!(\"axis_request_duration_seconds\", \"Request latency\"),").unwrap();
    writeln!(out, "        &[\"flow\", \"method\"],").unwrap();
    writeln!(out, "    ).unwrap();").unwrap();
    writeln!(
        out,
        "    prometheus::default_registry().register(Box::new(request_counter.clone())).unwrap();"
    )
    .unwrap();
    writeln!(
        out,
        "    prometheus::default_registry().register(Box::new(request_duration.clone())).unwrap();"
    )
    .unwrap();
    writeln!(out).unwrap();
    if auth_usage.needs_claims {
        writeln!(out, "    let jwt_secret = std::env::var(\"JWT_SECRET\").unwrap_or_else(|_| \"change-me\".into());").unwrap();
    }
    if auth_usage.api_key {
        writeln!(out, "    let api_key = std::env::var(\"API_KEY\").ok();").unwrap();
    }
    writeln!(out).unwrap();
    if has_any_rate_limit {
        writeln!(out, "    let rate_limiters = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));").unwrap();
    }
    if has_any_cache {
        writeln!(out, "    let response_cache = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));").unwrap();
    }
    if has_any_rate_limit || has_any_cache {
        writeln!(out, "    if std::env::var(\"REPLICAS\").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1) > 1 {{").unwrap();
        writeln!(out, "        tracing::warn!(\"rate_limiters and response_cache are per-process in-memory — not shared across replicas. Use an external store (Redis) for multi-instance deployments.\");").unwrap();
        writeln!(out, "    }}").unwrap();
    }
    let mut state_fields: Vec<String> = sources.iter().map(|(s, _)| format!("db_{s}")).collect();
    state_fields.extend(
        storages
            .iter()
            .filter(|storage| storage.backend == StorageBackend::S3)
            .map(|storage| format!("storage_{}", storage.name)),
    );
    state_fields.push("request_counter".into());
    state_fields.push("request_duration".into());
    if auth_usage.needs_claims {
        let pos = sources.len();
        state_fields.insert(pos, "jwt_secret".into());
    }
    if auth_usage.api_key {
        let pos = sources.len() + if auth_usage.needs_claims { 1 } else { 0 };
        state_fields.insert(pos, "api_key".into());
    }
    if has_any_rate_limit {
        state_fields.push("rate_limiters".into());
    }
    if has_any_cache {
        state_fields.push("response_cache".into());
    }
    writeln!(
        out,
        "    let state = Arc::new(AppState {{ {} }});",
        state_fields.join(", ")
    )
    .unwrap();
    writeln!(out, "    let app = build_router(state);").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "    let port = std::env::var(\"PORT\").unwrap_or_else(|_| \"8080\".into());"
    )
    .unwrap();
    writeln!(out, "    let addr = format!(\"0.0.0.0:{{}}\", port);").unwrap();
    writeln!(out, "    let tls_cert = std::env::var(\"TLS_CERT\").ok();").unwrap();
    writeln!(out, "    let tls_key = std::env::var(\"TLS_KEY\").ok();").unwrap();
    writeln!(out, "    let shutdown = async {{").unwrap();
    writeln!(out, "        let ctrl_c = tokio::signal::ctrl_c();").unwrap();
    writeln!(out, "        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();").unwrap();
    writeln!(out, "        tokio::select! {{").unwrap();
    writeln!(
        out,
        "            _ = ctrl_c => tracing::info!(\"received SIGINT, shutting down\"),"
    )
    .unwrap();
    writeln!(
        out,
        "            _ = sigterm.recv() => tracing::info!(\"received SIGTERM, shutting down\"),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(
        out,
        "    if let (Some(cert), Some(key)) = (tls_cert, tls_key) {{"
    )
    .unwrap();
    writeln!(
        out,
        "        tracing::info!(\"listening on {{}} (TLS)\", addr);"
    )
    .unwrap();
    writeln!(out, "        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key).await.expect(\"failed to load TLS cert/key\");").unwrap();
    writeln!(out, "        let handle = axum_server::Handle::new();").unwrap();
    writeln!(out, "        let h = handle.clone();").unwrap();
    writeln!(out, "        tokio::spawn(async move {{ shutdown.await; h.graceful_shutdown(Some(std::time::Duration::from_secs(10))); }});").unwrap();
    writeln!(
        out,
        "        axum_server::bind_rustls(addr.parse().unwrap(), tls_config)"
    )
    .unwrap();
    writeln!(out, "            .handle(handle)").unwrap();
    writeln!(out, "            .serve(app.into_make_service())").unwrap();
    writeln!(out, "            .await.unwrap();").unwrap();
    writeln!(out, "    }} else {{").unwrap();
    writeln!(out, "        tracing::info!(\"listening on {{}}\", addr);").unwrap();
    writeln!(
        out,
        "        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();"
    )
    .unwrap();
    writeln!(out, "        axum::serve(listener, app)").unwrap();
    writeln!(out, "            .with_graceful_shutdown(shutdown)").unwrap();
    writeln!(out, "            .await.unwrap();").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    tracing::info!(\"server stopped\");").unwrap();
    writeln!(out, "}}").unwrap();

    out
}

fn generate_idempotency_helpers(out: &mut String) {
    writeln!(out, "fn axis_idempotency_scalar(value: serde_json::Value, label: &str, max_len: usize) -> Result<String, axum::response::Response> {{").unwrap();
    writeln!(out, "    let value = match value {{").unwrap();
    writeln!(out, "        serde_json::Value::String(value) => value,").unwrap();
    writeln!(
        out,
        "        serde_json::Value::Number(value) => value.to_string(),"
    )
    .unwrap();
    writeln!(
        out,
        "        serde_json::Value::Bool(value) => value.to_string(),"
    )
    .unwrap();
    writeln!(out, "        _ => return Err(api_error(StatusCode::BAD_REQUEST, &format!(\"{{label}} must be a string, number, or boolean\"))),").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    let value = value.trim();").unwrap();
    writeln!(out, "    if value.is_empty() || value.len() > max_len {{").unwrap();
    writeln!(out, "        return Err(api_error(StatusCode::BAD_REQUEST, &format!(\"{{label}} must contain 1..={{max_len}} bytes\")));").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    Ok(value.to_string())").unwrap();
    writeln!(out, "}}").unwrap();
}

fn generate_idempotency_begin(
    out: &mut String,
    flow: &FlowDef,
    route: &codegen::RouteInfo,
    declaration: &IdempotencyDecl,
    source: &str,
) {
    let dialect = source_dialect(source);
    let key = lower_dot_path(&declaration.key);
    let scope = lower_dot_path(&declaration.scope);
    let body = if flow.body.is_some() {
        "serde_json::to_value(&body).unwrap_or(serde_json::Value::Null)"
    } else {
        "serde_json::Value::Null"
    };
    let path = if flow.path.contains(':') {
        "serde_json::to_value(&path).unwrap_or(serde_json::Value::Null)"
    } else {
        "serde_json::Value::Null"
    };
    let has_query = !collect_query_refs(flow).is_empty()
        || flow.steps.iter().any(|step| {
            matches!(
                step,
                FlowStep::Let(LetStep {
                    expr: Expr::Query {
                        page_size: Some(_),
                        ..
                    },
                    ..
                })
            )
        });
    let query = if has_query {
        "serde_json::to_value(&query).unwrap_or(serde_json::Value::Null)"
    } else {
        "serde_json::Value::Null"
    };
    let ttl_seconds = declaration.ttl;
    let insert_sql = match dialect {
        Dialect::Postgres => {
            "INSERT INTO _axis_idempotency (flow_name, scope_key, idempotency_key, request_hash, state, expires_at, created_at, updated_at) VALUES ($1, $2, $3, $4, 'processing', $5, $6, $6) ON CONFLICT (flow_name, scope_key, idempotency_key) DO NOTHING"
        }
        Dialect::Mysql => {
            "INSERT INTO _axis_idempotency (flow_name, scope_key, idempotency_key, request_hash, state, expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, 'processing', ?, ?, ?) ON DUPLICATE KEY UPDATE updated_at = updated_at"
        }
        Dialect::Sqlite => {
            "INSERT INTO _axis_idempotency (flow_name, scope_key, idempotency_key, request_hash, state, expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, 'processing', ?, ?, ?) ON CONFLICT (flow_name, scope_key, idempotency_key) DO NOTHING"
        }
    };
    let delete_sql = match dialect {
        Dialect::Postgres => {
            "DELETE FROM _axis_idempotency WHERE flow_name = $1 AND scope_key = $2 AND idempotency_key = $3 AND expires_at <= $4"
        }
        Dialect::Mysql | Dialect::Sqlite => {
            "DELETE FROM _axis_idempotency WHERE flow_name = ? AND scope_key = ? AND idempotency_key = ? AND expires_at <= ?"
        }
    };
    let select_sql = match dialect {
        Dialect::Postgres => {
            "SELECT request_hash, state, response_status, response_body, response_headers FROM _axis_idempotency WHERE flow_name = $1 AND scope_key = $2 AND idempotency_key = $3 FOR UPDATE"
        }
        Dialect::Mysql => {
            "SELECT request_hash, state, response_status, response_body, response_headers FROM _axis_idempotency WHERE flow_name = ? AND scope_key = ? AND idempotency_key = ? FOR UPDATE"
        }
        Dialect::Sqlite => {
            "SELECT request_hash, state, response_status, response_body, response_headers FROM _axis_idempotency WHERE flow_name = ? AND scope_key = ? AND idempotency_key = ?"
        }
    };

    writeln!(out, "    let _axis_idempotency_key = match axis_idempotency_scalar({key}, \"idempotency key\", 255) {{ Ok(value) => value, Err(response) => return response }};").unwrap();
    writeln!(out, "    let _axis_idempotency_scope = match axis_idempotency_scalar({scope}, \"idempotency scope\", 512) {{ Ok(value) => value, Err(response) => return response }};").unwrap();
    writeln!(out, "    let _axis_fingerprint = serde_json::json!({{").unwrap();
    writeln!(out, "        \"flow\": \"{}\", \"method\": \"{}\", \"path\": {path}, \"query\": {query}, \"body\": {body}", flow.name, route.method).unwrap();
    writeln!(out, "    }});").unwrap();
    writeln!(out, "    let _axis_request_hash = hex::encode(sha2::Sha256::digest(serde_json::to_vec(&_axis_fingerprint).expect(\"JSON serialization cannot fail\")));").unwrap();
    writeln!(out, "    let _axis_now = chrono::Utc::now().timestamp();").unwrap();
    writeln!(
        out,
        "    let _axis_expires = _axis_now.saturating_add({ttl_seconds});"
    )
    .unwrap();
    writeln!(
        out,
        "    let mut _axis_tx = match state.db_{source}.begin().await {{"
    )
    .unwrap();
    writeln!(out, "        Ok(transaction) => transaction,").unwrap();
    writeln!(out, "        Err(error) => {{ tracing::error!(%error, \"failed to begin idempotent transaction\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"internal server error\"); }}").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    db_try!(sqlx::query(\"{delete_sql}\")").unwrap();
    writeln!(out, "        .bind(\"{}\").bind(&_axis_idempotency_scope).bind(&_axis_idempotency_key).bind(_axis_now)", flow.name).unwrap();
    writeln!(out, "        .execute(&mut *_axis_tx).await);").unwrap();
    writeln!(
        out,
        "    let _axis_reservation = db_try!(sqlx::query(\"{insert_sql}\")"
    )
    .unwrap();
    writeln!(out, "        .bind(\"{}\").bind(&_axis_idempotency_scope).bind(&_axis_idempotency_key).bind(&_axis_request_hash).bind(_axis_expires).bind(_axis_now)", flow.name).unwrap();
    if matches!(dialect, Dialect::Mysql | Dialect::Sqlite) {
        writeln!(out, "        .bind(_axis_now)").unwrap();
    }
    writeln!(out, "        .execute(&mut *_axis_tx).await);").unwrap();
    writeln!(out, "    if _axis_reservation.rows_affected() == 0 {{").unwrap();
    writeln!(out, "        let _axis_stored: Option<(String, String, Option<i64>, Option<String>, Option<String>)> = db_try!(sqlx::query_as(\"{select_sql}\")").unwrap();
    writeln!(
        out,
        "            .bind(\"{}\").bind(&_axis_idempotency_scope).bind(&_axis_idempotency_key)",
        flow.name
    )
    .unwrap();
    writeln!(out, "            .fetch_optional(&mut *_axis_tx).await);").unwrap();
    writeln!(out, "        let Some((stored_hash, stored_state, stored_status, stored_body, stored_headers)) = _axis_stored else {{").unwrap();
    writeln!(out, "            let _ = _axis_tx.rollback().await;").unwrap();
    writeln!(out, "            return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"idempotency reservation disappeared\");").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        if stored_hash != _axis_request_hash {{").unwrap();
    writeln!(out, "            let _ = _axis_tx.rollback().await;").unwrap();
    writeln!(out, "            return api_error(StatusCode::CONFLICT, \"idempotency key was already used with a different request\");").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        if stored_state != \"completed\" {{").unwrap();
    writeln!(out, "            let _ = _axis_tx.rollback().await;").unwrap();
    writeln!(out, "            return api_error(StatusCode::CONFLICT, \"idempotent request is still in progress\");").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        let Some(status) = stored_status.and_then(|value| u16::try_from(value).ok()).and_then(|value| StatusCode::from_u16(value).ok()) else {{").unwrap();
    writeln!(out, "            let _ = _axis_tx.rollback().await;").unwrap();
    writeln!(out, "            return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"stored idempotency response has an invalid status\");").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        let Some(body) = stored_body else {{ let _ = _axis_tx.rollback().await; return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"stored idempotency response is incomplete\"); }};").unwrap();
    writeln!(out, "        let stored_headers: std::collections::BTreeMap<String, String> = stored_headers.as_deref().and_then(|value| serde_json::from_str(value).ok()).unwrap_or_default();").unwrap();
    writeln!(out, "        let _ = _axis_tx.rollback().await;").unwrap();
    writeln!(out, "        let mut response = axum::response::Response::builder().status(status).body(axum::body::Body::from(body)).expect(\"valid stored response\");").unwrap();
    writeln!(out, "        for (name, value) in stored_headers {{").unwrap();
    writeln!(out, "            if let (Ok(name), Ok(value)) = (name.parse::<axum::http::HeaderName>(), value.parse::<axum::http::HeaderValue>()) {{ response.headers_mut().insert(name, value); }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        response.headers_mut().insert(\"idempotency-replayed\", axum::http::HeaderValue::from_static(\"true\"));").unwrap();
    writeln!(out, "        state.request_counter.with_label_values(&[\"{}\", \"{}\", &status.as_u16().to_string()]).inc();", flow.name, route.method).unwrap();
    writeln!(out, "        state.request_duration.with_label_values(&[\"{}\", \"{}\"]).observe(_start.elapsed().as_secs_f64());", flow.name, route.method).unwrap();
    writeln!(out, "        return response;").unwrap();
    writeln!(out, "    }}").unwrap();
}

fn generate_idempotency_finish(
    out: &mut String,
    flow: &FlowDef,
    _declaration: &IdempotencyDecl,
    source: &str,
) {
    let dialect = source_dialect(source);
    let update_sql = match dialect {
        Dialect::Postgres => {
            "UPDATE _axis_idempotency SET state = 'completed', response_status = $1, response_body = $2, response_headers = $3, updated_at = $4 WHERE flow_name = $5 AND scope_key = $6 AND idempotency_key = $7 AND request_hash = $8 AND state = 'processing'"
        }
        Dialect::Mysql | Dialect::Sqlite => {
            "UPDATE _axis_idempotency SET state = 'completed', response_status = ?, response_body = ?, response_headers = ?, updated_at = ? WHERE flow_name = ? AND scope_key = ? AND idempotency_key = ? AND request_hash = ? AND state = 'processing'"
        }
    };
    writeln!(out, "    _resp.headers_mut().insert(\"idempotency-replayed\", axum::http::HeaderValue::from_static(\"false\"));").unwrap();
    writeln!(
        out,
        "    let (_axis_response_parts, _axis_response_body) = _resp.into_parts();"
    )
    .unwrap();
    writeln!(out, "    let _axis_response_bytes = match axum::body::to_bytes(_axis_response_body, 16 * 1024 * 1024).await {{").unwrap();
    writeln!(out, "        Ok(bytes) => bytes,").unwrap();
    writeln!(out, "        Err(error) => {{ tracing::error!(%error, \"failed to buffer idempotent response\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"internal server error\"); }}").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    let _axis_response_headers: std::collections::BTreeMap<String, String> = _axis_response_parts.headers.iter().filter_map(|(name, value)| value.to_str().ok().map(|value| (name.as_str().to_string(), value.to_string()))).collect();").unwrap();
    writeln!(out, "    let _axis_response_headers = serde_json::to_string(&_axis_response_headers).expect(\"response headers are serializable\");").unwrap();
    writeln!(out, "    let _axis_response_body = String::from_utf8(_axis_response_bytes.to_vec()).unwrap_or_else(|_| String::from_utf8_lossy(&_axis_response_bytes).into_owned());").unwrap();
    writeln!(
        out,
        "    let _axis_completion = db_try!(sqlx::query(\"{update_sql}\")"
    )
    .unwrap();
    writeln!(out, "        .bind(i64::from(_axis_response_parts.status.as_u16())).bind(&_axis_response_body).bind(&_axis_response_headers).bind(chrono::Utc::now().timestamp())").unwrap();
    writeln!(out, "        .bind(\"{}\").bind(&_axis_idempotency_scope).bind(&_axis_idempotency_key).bind(&_axis_request_hash)", flow.name).unwrap();
    writeln!(out, "        .execute(&mut *_axis_tx).await);").unwrap();
    writeln!(out, "    if _axis_completion.rows_affected() != 1 {{ let _ = _axis_tx.rollback().await; return api_error(StatusCode::CONFLICT, \"idempotency reservation was lost\"); }}").unwrap();
    writeln!(out, "    if let Err(error) = _axis_tx.commit().await {{ tracing::error!(%error, \"failed to commit idempotent transaction\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"internal server error\"); }}").unwrap();
    writeln!(out, "    let _resp = axum::response::Response::from_parts(_axis_response_parts, axum::body::Body::from(_axis_response_body));").unwrap();
}

fn generate_flow_handler(out: &mut String, route: &codegen::RouteInfo, flows: &[&FlowDef]) {
    let flow = match flows.iter().find(|f| f.name == route.name) {
        Some(f) => f,
        None => return,
    };

    let has_idempotency = flow.idempotency.is_some();
    let is_multipart = flow
        .body
        .as_ref()
        .is_some_and(|body| body.kind == BodyKind::Multipart);
    let derives = if has_idempotency {
        "#[derive(Deserialize, Serialize)]"
    } else {
        "#[derive(Deserialize)]"
    };

    // Generate path params struct if needed
    if flow.path.contains(':') {
        let params: Vec<&str> = flow
            .path
            .split('/')
            .filter(|s| s.starts_with(':'))
            .map(|s| &s[1..])
            .collect();
        writeln!(out, "{derives}").unwrap();
        writeln!(out, "struct {name}PathParams {{", name = pascal(&flow.name)).unwrap();
        for p in &params {
            let ty = path_param_rust_type(p);
            writeln!(out, "    {p}: {ty},").unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    let has_paginated_query = flow.steps.iter().any(|s| {
        if let FlowStep::Let(l) = s {
            if let Expr::Query { page_size, .. } = &l.expr {
                return page_size.is_some();
            }
        }
        false
    });

    let referenced_params = collect_query_refs(flow);
    let used_params: Vec<&ParamDecl> = flow
        .params
        .iter()
        .filter(|p| referenced_params.contains(&p.name))
        .collect();

    if !used_params.is_empty() || has_paginated_query {
        writeln!(out, "{derives}").unwrap();
        writeln!(
            out,
            "struct {name}QueryParams {{",
            name = pascal(&flow.name)
        )
        .unwrap();
        for p in &used_params {
            let ty = rust_type_for(&p.ty);
            writeln!(out, "    {}: Option<{ty}>,", p.name).unwrap();
        }
        if has_paginated_query {
            writeln!(out, "    page: Option<i64>,").unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    // Generate body struct if needed
    if let Some(body) = &flow.body {
        writeln!(out, "{derives}").unwrap();
        writeln!(out, "struct {name}Body {{", name = pascal(&flow.name)).unwrap();
        for f in &body.fields {
            let ty = rust_type_for(&f.ty);
            let optional = f.modifiers.iter().any(|m| matches!(m, Modifier::Required));
            if optional {
                writeln!(out, "    {}: {ty},", f.name).unwrap();
            } else {
                writeln!(out, "    {}: Option<{ty}>,", f.name).unwrap();
            }
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    // Handler signature
    let mut params_list = vec!["State(state): State<Arc<AppState>>".to_string()];
    let has_auth = flow.auth.is_some() && !matches!(flow.auth, Some(AuthDecl::None));
    if (has_auth || has_idempotency) && !is_multipart {
        params_list.push("headers: HeaderMap".to_string());
    }
    if flow.path.contains(':') {
        params_list.push(format!(
            "Path(path): Path<{}PathParams>",
            pascal(&flow.name)
        ));
    }
    if !used_params.is_empty() || has_paginated_query {
        params_list.push(format!(
            "Query(query): Query<{}QueryParams>",
            pascal(&flow.name)
        ));
    }
    let is_webhook = matches!(flow.auth, Some(AuthDecl::WebhookSignature { .. }));
    if let Some(body) = &flow.body {
        if body.kind == BodyKind::Multipart {
            params_list.push("request: axum::extract::Request".to_string());
        } else if is_webhook {
            params_list.push("bytes: axum::body::Bytes".to_string());
        } else {
            let needs_mut = body
                .fields
                .iter()
                .any(|f| matches!(&f.ty, TypeExpr::String(_) | TypeExpr::Text));
            let kw = if needs_mut { "mut " } else { "" };
            params_list.push(format!("Json({kw}body): Json<{}Body>", pascal(&flow.name)));
        }
    }

    writeln!(
        out,
        "async fn handle_{name}({params}) -> impl IntoResponse {{",
        name = flow.name,
        params = params_list.join(", "),
    )
    .unwrap();

    // Generate step-by-step execution
    writeln!(out, "    let _start = std::time::Instant::now();").unwrap();
    if let Some(body) = &flow.body {
        if body.kind == BodyKind::Multipart {
            if has_auth || has_idempotency {
                writeln!(out, "    let headers = request.headers().clone();").unwrap();
            }
            generate_multipart_body_parse(out, flow, body);
        }
    }

    // Auth verification
    if let Some(auth) = &flow.auth {
        match auth {
            AuthDecl::Session => {
                writeln!(
                    out,
                    "    let auth = match extract_session(&state, &headers).await {{"
                )
                .unwrap();
                writeln!(out, "        Ok(claims) => claims,").unwrap();
                writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
                writeln!(out, "    }};").unwrap();
            }
            AuthDecl::Bearer => {
                writeln!(
                    out,
                    "    let auth = match extract_bearer(&state, &headers) {{"
                )
                .unwrap();
                writeln!(out, "        Ok(claims) => claims,").unwrap();
                writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
                writeln!(out, "    }};").unwrap();
            }
            AuthDecl::ApiKey => {
                writeln!(
                    out,
                    "    if let Err(e) = verify_api_key(&state, &headers) {{"
                )
                .unwrap();
                writeln!(out, "        return e.into_response();").unwrap();
                writeln!(out, "    }}").unwrap();
            }
            AuthDecl::Role(role) => {
                writeln!(
                    out,
                    "    let auth = match extract_bearer(&state, &headers) {{"
                )
                .unwrap();
                writeln!(out, "        Ok(claims) => claims,").unwrap();
                writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
                writeln!(out, "    }};").unwrap();
                writeln!(out, "    if auth.role.as_deref() != Some(\"{role}\") {{").unwrap();
                writeln!(
                    out,
                    "        return api_error(StatusCode::FORBIDDEN, \"insufficient role\");"
                )
                .unwrap();
                writeln!(out, "    }}").unwrap();
            }
            AuthDecl::RoleIn(roles) => {
                writeln!(
                    out,
                    "    let auth = match extract_bearer(&state, &headers) {{"
                )
                .unwrap();
                writeln!(out, "        Ok(claims) => claims,").unwrap();
                writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
                writeln!(out, "    }};").unwrap();
                let roles_str: Vec<String> = roles.iter().map(|r| format!("\"{r}\"")).collect();
                writeln!(out, "    let allowed_roles = [{}];", roles_str.join(", ")).unwrap();
                writeln!(out, "    if !auth.role.as_ref().map_or(false, |r| allowed_roles.contains(&r.as_str())) {{").unwrap();
                writeln!(
                    out,
                    "        return api_error(StatusCode::FORBIDDEN, \"insufficient role\");"
                )
                .unwrap();
                writeln!(out, "    }}").unwrap();
            }
            AuthDecl::WebhookSignature { algorithm, .. } => {
                let algo = algorithm.to_uppercase();
                if flow.body.is_some() {
                    writeln!(out, "    if let Err(e) = verify_webhook_signature(&headers, &bytes, \"{algo}\") {{").unwrap();
                } else {
                    writeln!(
                        out,
                        "    if let Err(e) = verify_webhook_signature(&headers, &[], \"{algo}\") {{"
                    )
                    .unwrap();
                }
                writeln!(out, "        return e.into_response();").unwrap();
                writeln!(out, "    }}").unwrap();
                if flow.body.is_some() {
                    writeln!(
                        out,
                        "    let body: {name}Body = match serde_json::from_slice(&bytes) {{",
                        name = pascal(&flow.name)
                    )
                    .unwrap();
                    writeln!(out, "        Ok(b) => b,").unwrap();
                    writeln!(out, "        Err(e) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid JSON: {{}}\", e)),").unwrap();
                    writeln!(out, "    }};").unwrap();
                }
            }
            AuthDecl::None => {}
        }
    }

    // Rate limiting enforcement
    let has_rate_limit = !flow.limits.is_empty();
    if has_rate_limit {
        writeln!(out, "    let mut _rl_limit: u64 = 0;").unwrap();
        writeln!(out, "    let mut _rl_remaining: u64 = 0;").unwrap();
        writeln!(out, "    let mut _rl_reset: u64 = 0;").unwrap();
    }
    for limit in &flow.limits {
        let window_secs = match limit.unit {
            RateUnit::PerSecond => 1,
            RateUnit::PerMinute => 60,
            RateUnit::PerHour => 3600,
            RateUnit::PerDay => 86400,
        };
        let key_expr = match &limit.scope {
            RateScope::Global => format!("\"rl:{}:global\".to_string()", flow.name),
            RateScope::PerUser => format!("format!(\"rl:{}:user:{{}}\", auth.sub)", flow.name),
            RateScope::PerIp => format!(
                "format!(\"rl:{}:ip:{{:?}}\", headers.get(\"x-forwarded-for\").and_then(|v| v.to_str().ok()).unwrap_or(\"unknown\"))",
                flow.name
            ),
            RateScope::PerKey => format!(
                "format!(\"rl:{}:key:{{:?}}\", headers.get(\"x-api-key\").and_then(|v| v.to_str().ok()).unwrap_or(\"unknown\"))",
                flow.name
            ),
        };
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        let mut limiters = state.rate_limiters.lock().unwrap_or_else(|e| e.into_inner());").unwrap();
        writeln!(out, "        if limiters.len() > 10000 {{").unwrap();
        writeln!(out, "            limiters.retain(|_, (_, ws)| ws.elapsed() < std::time::Duration::from_secs({window_secs} * 2));").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        let key = {key_expr};").unwrap();
        writeln!(out, "        let (count, window_start) = limiters.entry(key).or_insert((0, std::time::Instant::now()));").unwrap();
        writeln!(
            out,
            "        if window_start.elapsed() > std::time::Duration::from_secs({window_secs}) {{"
        )
        .unwrap();
        writeln!(out, "            *count = 0;").unwrap();
        writeln!(
            out,
            "            *window_start = std::time::Instant::now();"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        *count += 1;").unwrap();
        let count = limit.count;
        writeln!(out, "        _rl_limit = {count};").unwrap();
        writeln!(
            out,
            "        _rl_remaining = {count}_u64.saturating_sub(*count);"
        )
        .unwrap();
        writeln!(out, "        _rl_reset = {window_secs}_u64.saturating_sub(window_start.elapsed().as_secs());").unwrap();
        writeln!(out, "        if *count > {count} {{").unwrap();
        writeln!(out, "            let mut resp = api_error(StatusCode::TOO_MANY_REQUESTS, \"rate limit exceeded\");").unwrap();
        writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-limit\", _rl_limit.to_string().parse().unwrap());").unwrap();
        writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-remaining\", \"0\".parse().unwrap());").unwrap();
        writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-reset\", _rl_reset.to_string().parse().unwrap());").unwrap();
        writeln!(out, "            resp.headers_mut().insert(\"retry-after\", _rl_reset.to_string().parse().unwrap());").unwrap();
        writeln!(out, "            return resp;").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
    }

    // Cache check
    let has_cache = !flow.cache.is_empty();
    if has_cache {
        let cache = &flow.cache[0];
        let vary_parts: Vec<String> = cache.vary.iter().map(|p| {
            let lowered = lower_dot_path(p);
            format!("{{ let _v = {lowered}; _v.as_str().map(|s| s.to_string()).unwrap_or_else(|| _v.to_string()) }}")
        }).collect();
        let cache_key = if vary_parts.is_empty() {
            format!("\"cache:{}\".to_string()", flow.name)
        } else {
            format!(
                "format!(\"cache:{}:{{}}\", vec![{}].join(\":\"))",
                flow.name,
                vary_parts.join(", ")
            )
        };
        writeln!(out, "    let _cache_key = {cache_key};").unwrap();
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        let mut cache = state.response_cache.lock().unwrap_or_else(|e| e.into_inner());").unwrap();
        writeln!(
            out,
            "        let _cache_hit = cache.get(&_cache_key).and_then(|(val, status, created)| {{"
        )
        .unwrap();
        writeln!(
            out,
            "            if created.elapsed() < std::time::Duration::from_secs({ttl}) {{",
            ttl = cache.ttl
        )
        .unwrap();
        writeln!(out, "                Some((val.clone(), *status))").unwrap();
        writeln!(out, "            }} else {{ None }}").unwrap();
        writeln!(out, "        }});").unwrap();
        writeln!(out, "        if let Some((val, status)) = _cache_hit {{").unwrap();
        if has_rate_limit {
            writeln!(out, "            let mut resp = (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(val)).into_response();").unwrap();
            writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-limit\", _rl_limit.to_string().parse().unwrap());").unwrap();
            writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-remaining\", _rl_remaining.to_string().parse().unwrap());").unwrap();
            writeln!(out, "            resp.headers_mut().insert(\"x-ratelimit-reset\", _rl_reset.to_string().parse().unwrap());").unwrap();
            writeln!(out, "            return resp;").unwrap();
        } else {
            writeln!(out, "            return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(val)).into_response();").unwrap();
        }
        writeln!(out, "        }} else if cache.contains_key(&_cache_key) {{").unwrap();
        writeln!(out, "            cache.remove(&_cache_key);").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
    }

    // Body validation
    if let Some(body) = &flow.body {
        generate_body_validation(out, body, 4);
    }

    FLOW_SCOPE.with(|s| *s.borrow_mut() = collect_flow_variables(&flow.steps));
    BODY_FIELDS.with(|bf| {
        let mut m = bf.borrow_mut();
        m.clear();
        if let Some(body) = &flow.body {
            for f in &body.fields {
                m.insert(f.name.clone(), f.ty.clone());
            }
        }
    });
    QUERY_PARAMS.with(|qp| {
        let mut m = qp.borrow_mut();
        m.clear();
        for p in &flow.params {
            m.insert(p.name.clone(), p.ty.clone());
        }
    });

    let idempotency_source = if flow.idempotency.is_some() {
        let mut mutated_sources = HashSet::new();
        collect_mutated_sources(&flow.steps, &mut mutated_sources);
        let mut mutated_sources: Vec<String> = mutated_sources.into_iter().collect();
        mutated_sources.sort();
        mutated_sources.into_iter().next()
    } else {
        None
    };

    if let (Some(declaration), Some(source)) = (&flow.idempotency, idempotency_source.as_deref()) {
        generate_idempotency_begin(out, flow, route, declaration, source);
        DB_REF_OVERRIDE.with(|value| *value.borrow_mut() = Some("&mut *_axis_tx".to_string()));
        DB_DIALECT_OVERRIDE.with(|value| *value.borrow_mut() = Some(source_dialect(source)));
    }
    generate_flow_steps(out, &flow.steps, 4, "");
    DB_REF_OVERRIDE.with(|value| *value.borrow_mut() = None);
    DB_DIALECT_OVERRIDE.with(|value| *value.borrow_mut() = None);

    // Cache invalidation: clear entries for mutated sources
    let has_mutations = flow_has_mutations(&flow.steps);
    let any_flow_has_cache = flows.iter().any(|f| !f.cache.is_empty());
    if has_mutations && any_flow_has_cache {
        let mut mutated_sources = HashSet::new();
        collect_mutated_sources(&flow.steps, &mut mutated_sources);
        if !mutated_sources.is_empty() {
            writeln!(out, "    {{").unwrap();
            writeln!(out, "        let mut cache = state.response_cache.lock().unwrap_or_else(|e| e.into_inner());").unwrap();
            for src in &mutated_sources {
                writeln!(out, "        cache.retain(|k, _| !k.contains(\"{src}\"));").unwrap();
            }
            writeln!(out, "    }}").unwrap();
        }
    }

    // Cache store
    if has_cache {
        let cache_val = match &flow.return_stmt.body {
            Some(ReturnBody::Binding(name)) => name.clone(),
            _ => {
                writeln!(out, "    let _cache_val = serde_json::json!(null);").unwrap();
                "_cache_val".to_string()
            }
        };
        let status = flow.return_stmt.code;
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        let mut cache = state.response_cache.lock().unwrap_or_else(|e| e.into_inner());").unwrap();
        writeln!(out, "        cache.insert(_cache_key.clone(), ({cache_val}.clone(), {status}_u16, std::time::Instant::now()));").unwrap();
        writeln!(out, "        if cache.len() > 10000 {{").unwrap();
        writeln!(out, "            let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(60);").unwrap();
        writeln!(
            out,
            "            cache.retain(|_, (_, _, created)| *created > cutoff);"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
    }

    // Build response
    writeln!(out, "    let mut _resp = {{").unwrap();
    generate_return_stmt(out, &flow.return_stmt, 8);
    writeln!(out, "    }};").unwrap();

    // Metrics — record actual response status
    writeln!(out, "    state.request_counter.with_label_values(&[\"{name}\", \"{method}\", &_resp.status().as_u16().to_string()]).inc();",
        name = flow.name, method = route.method).unwrap();
    writeln!(out, "    state.request_duration.with_label_values(&[\"{name}\", \"{method}\"]).observe(_start.elapsed().as_secs_f64());",
        name = flow.name, method = route.method).unwrap();

    if has_cache {
        let ttl = flow.cache[0].ttl;
        writeln!(out, "    _resp.headers_mut().insert(\"cache-control\", \"public, max-age={ttl}\".parse().unwrap());").unwrap();
    }

    if has_rate_limit {
        writeln!(out, "    _resp.headers_mut().insert(\"x-ratelimit-limit\", _rl_limit.to_string().parse().unwrap());").unwrap();
        writeln!(out, "    _resp.headers_mut().insert(\"x-ratelimit-remaining\", _rl_remaining.to_string().parse().unwrap());").unwrap();
        writeln!(out, "    _resp.headers_mut().insert(\"x-ratelimit-reset\", _rl_reset.to_string().parse().unwrap());").unwrap();
    }
    if let (Some(declaration), Some(source)) = (&flow.idempotency, idempotency_source.as_deref()) {
        generate_idempotency_finish(out, flow, declaration, source);
    }
    writeln!(out, "    _resp").unwrap();

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn generate_stream_handler(out: &mut String, stream: &StreamDef) {
    let handler = format!("handle_stream_{}", stream.name);
    let has_auth = stream.auth.is_some() && !matches!(stream.auth, Some(AuthDecl::None));
    match stream.transport {
        StreamTransport::WebSocket => {
            writeln!(out, "async fn {handler}(").unwrap();
            if has_auth {
                writeln!(out, "    headers: HeaderMap,").unwrap();
            }
            writeln!(out, "    ws: axum::extract::WebSocketUpgrade,").unwrap();
            writeln!(out, "    State(_state): State<Arc<AppState>>,").unwrap();
            writeln!(out, ") -> impl IntoResponse {{").unwrap();
            if let Some(auth) = &stream.auth {
                generate_stream_auth(out, auth);
            }
            writeln!(out, "    ws.on_upgrade(|mut socket| async move {{").unwrap();
            writeln!(out, "        use axum::extract::ws::Message;").unwrap();
            writeln!(
                out,
                "        while let Some(Ok(msg)) = futures::StreamExt::next(&mut socket).await {{"
            )
            .unwrap();
            writeln!(out, "            if let Message::Text(text) = msg {{").unwrap();
            if stream.receivers.is_empty() {
                writeln!(out, "                let _ = futures::SinkExt::send(&mut socket, Message::Text(text.into())).await;").unwrap();
            } else {
                writeln!(out, "                if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&text) {{").unwrap();
                writeln!(out, "                    let _event_type = envelope[\"type\"].as_str().unwrap_or(\"\");").unwrap();
                writeln!(out, "                    match _event_type {{").unwrap();
                for receiver in &stream.receivers {
                    writeln!(out, "                        \"{}\" => {{", receiver.event).unwrap();
                    for step in &receiver.steps {
                        generate_flow_step(out, step, 28, "");
                    }
                    writeln!(out, "                            let _ = futures::SinkExt::send(&mut socket, Message::Text(").unwrap();
                    writeln!(out, "                                serde_json::json!({{\"type\": \"{}\", \"status\": \"ok\"}}).to_string().into()", receiver.event).unwrap();
                    writeln!(out, "                            )).await;").unwrap();
                    writeln!(out, "                        }}").unwrap();
                }
                writeln!(out, "                        _ => {{").unwrap();
                writeln!(out, "                            let _ = futures::SinkExt::send(&mut socket, Message::Text(").unwrap();
                writeln!(out, "                                serde_json::json!({{\"error\": \"unknown event\"}}).to_string().into()").unwrap();
                writeln!(out, "                            )).await;").unwrap();
                writeln!(out, "                        }}").unwrap();
                writeln!(out, "                    }}").unwrap();
                writeln!(out, "                }}").unwrap();
            }
            writeln!(out, "            }}").unwrap();
            writeln!(out, "        }}").unwrap();
            writeln!(out, "    }})").unwrap();
            writeln!(out, "}}").unwrap();
        }
        StreamTransport::Sse => {
            writeln!(out, "async fn {handler}(").unwrap();
            if has_auth {
                writeln!(out, "    headers: HeaderMap,").unwrap();
            }
            writeln!(out, "    State(_state): State<Arc<AppState>>,").unwrap();
            if has_auth {
                writeln!(out, ") -> impl IntoResponse {{").unwrap();
            } else {
                writeln!(out, ") -> axum::response::Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {{").unwrap();
            }
            if let Some(auth) = &stream.auth {
                generate_stream_auth(out, auth);
            }
            writeln!(
                out,
                "    let heartbeat = tokio_stream::wrappers::IntervalStream::new("
            )
            .unwrap();
            writeln!(
                out,
                "        tokio::time::interval(std::time::Duration::from_secs(30)),"
            )
            .unwrap();
            writeln!(out, "    );").unwrap();
            writeln!(
                out,
                "    let event_stream = tokio_stream::StreamExt::map(heartbeat, |_| {{"
            )
            .unwrap();
            writeln!(out, "        Ok(axum::response::sse::Event::default().event(\"heartbeat\").data(\"ping\"))").unwrap();
            writeln!(out, "    }});").unwrap();
            if has_auth {
                writeln!(out, "    axum::response::Sse::new(event_stream)").unwrap();
                writeln!(out, "        .keep_alive(axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15))).into_response()").unwrap();
            } else {
                writeln!(out, "    axum::response::Sse::new(event_stream)").unwrap();
                writeln!(out, "        .keep_alive(axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)))").unwrap();
            }
            writeln!(out, "}}").unwrap();
        }
    }
    writeln!(out).unwrap();
}

fn generate_stream_auth(out: &mut String, auth: &AuthDecl) {
    match auth {
        AuthDecl::Session => {
            writeln!(
                out,
                "    let _auth = match extract_session(&_state, &headers).await {{"
            )
            .unwrap();
            writeln!(out, "        Ok(claims) => claims,").unwrap();
            writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
            writeln!(out, "    }};").unwrap();
        }
        AuthDecl::Bearer => {
            writeln!(
                out,
                "    let _auth = match extract_bearer(&_state, &headers) {{"
            )
            .unwrap();
            writeln!(out, "        Ok(claims) => claims,").unwrap();
            writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
            writeln!(out, "    }};").unwrap();
        }
        AuthDecl::ApiKey => {
            writeln!(
                out,
                "    if let Err(e) = verify_api_key(&_state, &headers) {{"
            )
            .unwrap();
            writeln!(out, "        return e.into_response();").unwrap();
            writeln!(out, "    }}").unwrap();
        }
        AuthDecl::Role(role) => {
            writeln!(
                out,
                "    let _auth = match extract_bearer(&_state, &headers) {{"
            )
            .unwrap();
            writeln!(out, "        Ok(claims) => claims,").unwrap();
            writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
            writeln!(out, "    }};").unwrap();
            writeln!(out, "    if _auth.role.as_deref() != Some(\"{role}\") {{").unwrap();
            writeln!(out, "        return api_error(StatusCode::FORBIDDEN, \"insufficient role\").into_response();").unwrap();
            writeln!(out, "    }}").unwrap();
        }
        AuthDecl::RoleIn(roles) => {
            writeln!(
                out,
                "    let _auth = match extract_bearer(&_state, &headers) {{"
            )
            .unwrap();
            writeln!(out, "        Ok(claims) => claims,").unwrap();
            writeln!(out, "        Err(e) => return e.into_response(),").unwrap();
            writeln!(out, "    }};").unwrap();
            let roles_str: Vec<String> = roles.iter().map(|r| format!("\"{r}\"")).collect();
            writeln!(out, "    let allowed_roles = [{}];", roles_str.join(", ")).unwrap();
            writeln!(out, "    if !_auth.role.as_ref().map_or(false, |r| allowed_roles.contains(&r.as_str())) {{").unwrap();
            writeln!(out, "        return api_error(StatusCode::FORBIDDEN, \"insufficient role\").into_response();").unwrap();
            writeln!(out, "    }}").unwrap();
        }
        AuthDecl::WebhookSignature { .. } | AuthDecl::None => {}
    }
}

fn is_optional_query_ref(expr: &Expr) -> bool {
    matches!(expr, Expr::DotPath(dp) if dp.segments.first().map(|s| s.as_str()) == Some("query"))
}

fn is_body_ref(expr: &Expr) -> bool {
    matches!(expr, Expr::DotPath(dp) if dp.segments.first().map(|s| s.as_str()) == Some("body"))
}

fn sql_filters_for(filters: &[FilterClause], dialect: Dialect) -> String {
    if filters.is_empty() {
        return String::new();
    }
    let clauses: Vec<String> = filters
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let ph = dialect.ph(i + 1);
            let optional = is_optional_query_ref(&f.value);
            let clause = match f.op {
                FilterOp::Eq => format!("{} = {ph}", f.field),
                FilterOp::Neq => format!("{} != {ph}", f.field),
                FilterOp::Gt => format!("{} > {ph}", f.field),
                FilterOp::Gte => format!("{} >= {ph}", f.field),
                FilterOp::Lt => format!("{} < {ph}", f.field),
                FilterOp::Lte => format!("{} <= {ph}", f.field),
                FilterOp::In => {
                    if dialect == Dialect::Postgres {
                        format!("{} = ANY({ph})", f.field)
                    } else {
                        format!("{} IN ({ph})", f.field)
                    }
                }
                FilterOp::Like => format!("{} LIKE {ph}", f.field),
                FilterOp::StartsWith => format!("{} LIKE {ph} || '%'", f.field),
                FilterOp::Contains => format!("{} LIKE '%' || {ph} || '%'", f.field),
                FilterOp::Between => format!("{} >= {ph}", f.field),
            };
            if optional {
                format!("({ph} IS NULL OR {clause})")
            } else {
                clause
            }
        })
        .collect();
    format!(" WHERE {}", clauses.join(" AND "))
}

fn source_columns(source: &str) -> String {
    SOURCE_SHAPES.with(|ss| {
        let map = ss.borrow();
        match map.get(source) {
            Some(fields) if !fields.is_empty() => fields
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            _ => "*".to_string(),
        }
    })
}

fn path_param_rust_type(param: &str) -> &'static str {
    let is_uuid = SOURCE_SHAPES.with(|ss| {
        ss.borrow().values().any(|fields| {
            fields
                .iter()
                .any(|(n, ty)| n == param && matches!(ty, TypeExpr::Uuid))
        })
    });
    if is_uuid { "uuid::Uuid" } else { "String" }
}

fn axum_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{name}}}")
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn axum_method(method: &str) -> &'static str {
    match method.to_uppercase().as_str() {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "PATCH" => "patch",
        "DELETE" => "delete",
        "WEBHOOK" => "post",
        _ => "get",
    }
}

fn duration_to_secs(d: &Duration) -> u64 {
    match d.unit {
        DurationUnit::Milliseconds => (d.value as u64) / 1000,
        DurationUnit::Seconds => d.value as u64,
        DurationUnit::Minutes => d.value as u64 * 60,
        DurationUnit::Hours => d.value as u64 * 3600,
    }
}

fn compute_body_limit(body: &BodyDecl) -> usize {
    let mut total: usize = 256; // JSON overhead (braces, quotes, commas)
    for f in &body.fields {
        total += f.name.len() + 8; // key + quotes + colon
        total += match &f.ty {
            TypeExpr::String(Some(max)) => *max as usize + 2,
            TypeExpr::String(None) | TypeExpr::Text => 10_000,
            TypeExpr::Int { .. } => 20,
            TypeExpr::Decimal { .. } => 30,
            TypeExpr::Bool => 5,
            TypeExpr::Uuid => 40,
            TypeExpr::Date | TypeExpr::Timestamp => 40,
            TypeExpr::Json => 100_000,
            TypeExpr::Blob => 1_000_000,
            TypeExpr::Enum(variants) => variants.iter().map(|v| v.len()).max().unwrap_or(50) + 2,
            TypeExpr::List(_) => 100_000,
            TypeExpr::Map(_, _) => 100_000,
            TypeExpr::Maybe(inner) => match inner.as_ref() {
                TypeExpr::String(Some(max)) => *max as usize + 2,
                TypeExpr::String(None) | TypeExpr::Text => 10_000,
                _ => 100,
            },
            TypeExpr::Ref { .. } => 40,
        };
    }
    total.max(1024) // minimum 1KB
}

fn pascal(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn collect_query_refs(flow: &FlowDef) -> HashSet<String> {
    let mut refs = HashSet::new();
    if let Some(idempotency) = &flow.idempotency {
        collect_query_refs_dotpath(&idempotency.key, &mut refs);
        collect_query_refs_dotpath(&idempotency.scope, &mut refs);
    }
    for step in &flow.steps {
        collect_query_refs_step(step, &mut refs);
    }
    if let Some(body) = &flow.return_stmt.body {
        match body {
            ReturnBody::Paginated {
                items,
                total,
                cursor,
                has_more,
            } => {
                collect_query_refs_expr(items, &mut refs);
                collect_query_refs_expr(total, &mut refs);
                collect_query_refs_expr(cursor, &mut refs);
                collect_query_refs_expr(has_more, &mut refs);
            }
            ReturnBody::Inline(fields) => {
                for f in fields {
                    collect_query_refs_return_value(&f.value, &mut refs);
                }
            }
            ReturnBody::Binding(_) => {}
        }
    }
    for (_, expr) in &flow.return_stmt.headers {
        collect_query_refs_expr(expr, &mut refs);
    }
    for c in &flow.cache {
        for v in &c.vary {
            if v.segments.first().map(|s| s.as_str()) == Some("query") {
                if let Some(name) = v.segments.get(1) {
                    refs.insert(name.clone());
                }
            }
        }
    }
    refs
}

fn collect_query_refs_return_value(val: &ReturnValue, refs: &mut HashSet<String>) {
    match val {
        ReturnValue::Expr(e) => collect_query_refs_expr(e, refs),
        ReturnValue::Nested(fields) => {
            for f in fields {
                collect_query_refs_return_value(&f.value, refs);
            }
        }
    }
}

fn collect_query_refs_step(step: &FlowStep, refs: &mut HashSet<String>) {
    match step {
        FlowStep::Let(l) => collect_query_refs_expr(&l.expr, refs),
        FlowStep::Set(s) => collect_query_refs_expr(&s.expr, refs),
        FlowStep::Guard(g) => collect_query_refs_expr(&g.expr, refs),
        FlowStep::Rule(r) => {
            for req in &r.requires {
                collect_query_refs_dotpath(&req.path, refs);
                collect_query_refs_expr(&req.value, refs);
            }
        }
        FlowStep::Insert(i) => {
            for (_, e) in &i.fields {
                collect_query_refs_expr(e, refs);
            }
        }
        FlowStep::Upsert(u) => {
            for (_, e) in &u.keys {
                collect_query_refs_expr(e, refs);
            }
            for s in &u.sets {
                collect_query_refs_expr(&s.value, refs);
            }
        }
        FlowStep::Update(u) => {
            for w in &u.wheres {
                collect_query_refs_expr(&w.value, refs);
            }
            for s in &u.sets {
                collect_query_refs_expr(&s.value, refs);
            }
        }
        FlowStep::Delete(d) => {
            for w in &d.wheres {
                collect_query_refs_expr(&w.value, refs);
            }
        }
        FlowStep::Effect(e) => {
            for f in &e.fields {
                match f {
                    EffectField::To(expr) | EffectField::Url(expr) => {
                        collect_query_refs_expr(expr, refs)
                    }
                    EffectField::Data(exprs) => {
                        for expr in exprs {
                            collect_query_refs_expr(expr, refs);
                        }
                    }
                    _ => {}
                }
            }
        }
        FlowStep::Match(m) => {
            for b in &m.branches {
                collect_query_refs_expr(&b.condition, refs);
                for s in &b.steps {
                    collect_query_refs_step(s, refs);
                }
            }
            if let Some(default) = &m.default {
                for s in default {
                    collect_query_refs_step(s, refs);
                }
            }
        }
        FlowStep::Each(e) => {
            collect_query_refs_expr(&e.source, refs);
            for s in &e.steps {
                collect_query_refs_step(s, refs);
            }
        }
        FlowStep::Fanout(f) => {
            collect_query_refs_expr(&f.source, refs);
            for (_, e) in &f.insert.fields {
                collect_query_refs_expr(e, refs);
            }
        }
        FlowStep::Try(t) => {
            for s in &t.body {
                collect_query_refs_step(s, refs);
            }
            for s in &t.recover {
                collect_query_refs_step(s, refs);
            }
        }
        FlowStep::Upload(u) => {
            collect_query_refs_expr(&u.file_expr, refs);
        }
    }
}

fn collect_query_refs_expr(expr: &Expr, refs: &mut HashSet<String>) {
    match expr {
        Expr::DotPath(dp) => collect_query_refs_dotpath(dp, refs),
        Expr::Unary { operand, .. } => collect_query_refs_expr(operand, refs),
        Expr::Binary { left, right, .. } => {
            collect_query_refs_expr(left, refs);
            collect_query_refs_expr(right, refs);
        }
        Expr::Ternary { a, b, c, .. } => {
            collect_query_refs_expr(a, refs);
            collect_query_refs_expr(b, refs);
            collect_query_refs_expr(c, refs);
        }
        Expr::If { cond, then, else_ } => {
            collect_query_refs_expr(cond, refs);
            collect_query_refs_expr(then, refs);
            collect_query_refs_expr(else_, refs);
        }
        Expr::Fetch { filters, .. } | Expr::Query { filters, .. } => {
            for f in filters {
                collect_query_refs_expr(&f.value, refs);
            }
            if let Expr::Query {
                page_size: Some(ps),
                cursor,
                ..
            } = expr
            {
                collect_query_refs_expr(ps, refs);
                if let Some(c) = cursor {
                    collect_query_refs_expr(c, refs);
                }
            }
        }
        Expr::Call { args, .. } => {
            for (_, e) in args {
                collect_query_refs_expr(e, refs);
            }
        }
        Expr::Aggregate { source, .. } => collect_query_refs_expr(source, refs),
        Expr::NowOffset { amount, .. } => collect_query_refs_expr(amount, refs),
        Expr::Coalesce { value, default } => {
            collect_query_refs_expr(value, refs);
            collect_query_refs_expr(default, refs);
        }
        Expr::Cached { expr: inner, .. } => collect_query_refs_expr(inner, refs),
        Expr::MapExpr { source, .. } => collect_query_refs_expr(source, refs),
        Expr::FilterExpr {
            source, condition, ..
        } => {
            collect_query_refs_expr(source, refs);
            collect_query_refs_expr(condition, refs);
        }
        Expr::ReduceExpr { source, .. } => collect_query_refs_expr(source, refs),
        Expr::SplitExpr { value, delimiter } => {
            collect_query_refs_expr(value, refs);
            collect_query_refs_expr(delimiter, refs);
        }
        Expr::ReplaceExpr { value, from, to } => {
            collect_query_refs_expr(value, refs);
            collect_query_refs_expr(from, refs);
            collect_query_refs_expr(to, refs);
        }
        Expr::FormatExpr { args, .. } | Expr::FuncCall { args, .. } => {
            for a in args {
                collect_query_refs_expr(a, refs);
            }
        }
        _ => {}
    }
}

fn collect_query_refs_dotpath(dp: &DotPath, refs: &mut HashSet<String>) {
    if dp.segments.first().map(|s| s.as_str()) == Some("query") {
        if let Some(name) = dp.segments.get(1) {
            refs.insert(name.clone());
        }
    }
}

fn collect_auth_usage(flows: &[&FlowDef], streams: &[&StreamDef]) -> AuthUsage {
    let mut usage = AuthUsage::default();
    for f in flows {
        if let Some(auth) = &f.auth {
            match auth {
                AuthDecl::Session => usage.session = true,
                AuthDecl::Bearer => usage.bearer = true,
                AuthDecl::ApiKey => usage.api_key = true,
                AuthDecl::Role(_) | AuthDecl::RoleIn(_) => usage.bearer = true,
                AuthDecl::WebhookSignature { .. } => usage.webhook = true,
                AuthDecl::None => {}
            }
        }
    }
    for s in streams {
        if let Some(auth) = &s.auth {
            match auth {
                AuthDecl::Session => usage.session = true,
                AuthDecl::Bearer => usage.bearer = true,
                AuthDecl::ApiKey => usage.api_key = true,
                AuthDecl::Role(_) | AuthDecl::RoleIn(_) => usage.bearer = true,
                AuthDecl::WebhookSignature { .. } => usage.webhook = true,
                AuthDecl::None => {}
            }
        }
    }
    usage.needs_claims = usage.session || usage.bearer;
    usage
}

#[derive(Default)]
struct AuthUsage {
    session: bool,
    bearer: bool,
    api_key: bool,
    webhook: bool,
    needs_claims: bool,
}

fn generate_auth_module(out: &mut String, usage: &AuthUsage) {
    if usage.needs_claims {
        writeln!(out, "#[derive(Debug, Clone, Serialize, Deserialize)]").unwrap();
        writeln!(out, "struct AuthClaims {{").unwrap();
        writeln!(out, "    sub: String,").unwrap();
        writeln!(out, "    role: Option<String>,").unwrap();
        writeln!(out, "    exp: usize,").unwrap();
        writeln!(out, "    #[serde(flatten)]").unwrap();
        writeln!(
            out,
            "    extra: std::collections::HashMap<String, serde_json::Value>,"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    if usage.bearer {
        writeln!(out, "fn extract_bearer(state: &AppState, headers: &HeaderMap) -> Result<AuthClaims, (StatusCode, Json<serde_json::Value>)> {{").unwrap();
        writeln!(out, "    let token = headers.get(\"authorization\")").unwrap();
        writeln!(out, "        .and_then(|v| v.to_str().ok())").unwrap();
        writeln!(out, "        .and_then(|v| v.strip_prefix(\"Bearer \"))").unwrap();
        writeln!(out, "        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"missing bearer token\"}}))))?;").unwrap();
        writeln!(
            out,
            "    let key = DecodingKey::from_secret(state.jwt_secret.as_bytes());"
        )
        .unwrap();
        writeln!(
            out,
            "    let data = decode::<AuthClaims>(token, &key, &Validation::new(Algorithm::HS256))"
        )
        .unwrap();
        writeln!(out, "        .map_err(|e| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": format!(\"invalid token: {{}}\", e)}}))))?;").unwrap();
        writeln!(out, "    Ok(data.claims)").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    if usage.session {
        writeln!(out, "async fn extract_session(state: &AppState, headers: &HeaderMap) -> Result<AuthClaims, (StatusCode, Json<serde_json::Value>)> {{").unwrap();
        writeln!(out, "    let token = headers.get(\"authorization\")").unwrap();
        writeln!(out, "        .and_then(|v| v.to_str().ok())").unwrap();
        writeln!(out, "        .and_then(|v| v.strip_prefix(\"Bearer \"))").unwrap();
        writeln!(out, "        .or_else(|| headers.get(\"cookie\")").unwrap();
        writeln!(out, "            .and_then(|v| v.to_str().ok())").unwrap();
        writeln!(
            out,
            "            .and_then(|v| v.split(';').find_map(|c| {{"
        )
        .unwrap();
        writeln!(out, "                let c = c.trim();").unwrap();
        writeln!(out, "                c.strip_prefix(\"session=\")").unwrap();
        writeln!(out, "            }})))").unwrap();
        writeln!(out, "        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"missing session\"}}))))?;").unwrap();
        writeln!(
            out,
            "    let key = DecodingKey::from_secret(state.jwt_secret.as_bytes());"
        )
        .unwrap();
        writeln!(
            out,
            "    let data = decode::<AuthClaims>(token, &key, &Validation::new(Algorithm::HS256))"
        )
        .unwrap();
        writeln!(out, "        .map_err(|e| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": format!(\"invalid session: {{}}\", e)}}))))?;").unwrap();
        writeln!(out, "    Ok(data.claims)").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    if usage.api_key {
        writeln!(out, "fn verify_api_key(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, Json<serde_json::Value>)> {{").unwrap();
        writeln!(out, "    let key = headers.get(\"x-api-key\")").unwrap();
        writeln!(out, "        .and_then(|v| v.to_str().ok())").unwrap();
        writeln!(out, "        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"missing API key\"}}))))?;").unwrap();
        writeln!(out, "    match &state.api_key {{").unwrap();
        writeln!(out, "        Some(expected) => {{").unwrap();
        writeln!(out, "            use hmac::{{Hmac, Mac}};").unwrap();
        writeln!(out, "            use sha2::Sha256;").unwrap();
        writeln!(out, "            let mut mac = Hmac::<Sha256>::new_from_slice(b\"api-key-verify\").unwrap();").unwrap();
        writeln!(out, "            mac.update(expected.as_bytes());").unwrap();
        writeln!(
            out,
            "            let expected_mac = mac.finalize().into_bytes();"
        )
        .unwrap();
        writeln!(out, "            let mut mac2 = Hmac::<Sha256>::new_from_slice(b\"api-key-verify\").unwrap();").unwrap();
        writeln!(out, "            mac2.update(key.as_bytes());").unwrap();
        writeln!(out, "            if mac2.verify(&expected_mac).is_ok() {{ Ok(()) }} else {{ Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"invalid API key\"}})))) }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        _ => Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"invalid API key\"}})))),").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    if usage.webhook {
        writeln!(out, "fn verify_webhook_signature(headers: &HeaderMap, body: &[u8], _algo: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {{").unwrap();
        writeln!(out, "    use hmac::{{Hmac, Mac}};").unwrap();
        writeln!(out, "    use sha2::Sha256;").unwrap();
        writeln!(out, "    let sig = headers.get(\"x-signature\")").unwrap();
        writeln!(out, "        .and_then(|v| v.to_str().ok())").unwrap();
        writeln!(out, "        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"missing signature\"}}))))?;").unwrap();
        writeln!(
            out,
            "    let secret = std::env::var(\"WEBHOOK_SECRET\").unwrap_or_default();"
        )
        .unwrap();
        writeln!(
            out,
            "    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();"
        )
        .unwrap();
        writeln!(out, "    mac.update(body);").unwrap();
        writeln!(out, "    let sig_bytes = hex::decode(sig).map_err(|_| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"invalid signature format\"}}))))?;").unwrap();
        writeln!(out, "    mac.verify_slice(&sig_bytes).map_err(|_| (StatusCode::UNAUTHORIZED, Json(serde_json::json!({{\"error\": \"invalid signature\"}}))))?;").unwrap();
        writeln!(out, "    Ok(())").unwrap();
        writeln!(out, "}}").unwrap();
    }
}

fn generate_cors_builder(out: &mut String) {
    writeln!(out, "fn build_cors() -> tower_http::cors::CorsLayer {{").unwrap();
    writeln!(
        out,
        "    use tower_http::cors::{{CorsLayer, AllowOrigin, AllowMethods, AllowHeaders}};"
    )
    .unwrap();
    writeln!(
        out,
        "    let origins = std::env::var(\"CORS_ORIGINS\").unwrap_or_else(|_| \"*\".into());"
    )
    .unwrap();
    writeln!(out, "    if origins == \"*\" {{").unwrap();
    writeln!(out, "        CorsLayer::permissive()").unwrap();
    writeln!(out, "    }} else {{").unwrap();
    writeln!(out, "        let origins: Vec<_> = origins.split(',').filter_map(|s| s.trim().parse().ok()).collect();").unwrap();
    writeln!(out, "        let methods = std::env::var(\"CORS_METHODS\").unwrap_or_else(|_| \"GET,POST,PUT,PATCH,DELETE,OPTIONS\".into());").unwrap();
    writeln!(out, "        let methods: Vec<axum::http::Method> = methods.split(',').filter_map(|s| s.trim().parse().ok()).collect();").unwrap();
    writeln!(out, "        let max_age = std::env::var(\"CORS_MAX_AGE\").ok().and_then(|v| v.parse().ok()).unwrap_or(3600u64);").unwrap();
    writeln!(out, "        CorsLayer::new()").unwrap();
    writeln!(out, "            .allow_origin(AllowOrigin::list(origins))").unwrap();
    writeln!(
        out,
        "            .allow_methods(AllowMethods::list(methods))"
    )
    .unwrap();
    writeln!(out, "            .allow_headers(AllowHeaders::any())").unwrap();
    writeln!(
        out,
        "            .max_age(std::time::Duration::from_secs(max_age))"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}

fn generate_api_error(out: &mut String) {
    writeln!(
        out,
        "fn api_error(status: StatusCode, message: &str) -> axum::response::Response {{"
    )
    .unwrap();
    writeln!(out, "    let error_type = match status.as_u16() {{").unwrap();
    writeln!(out, "        400 => \"BAD_REQUEST\",").unwrap();
    writeln!(out, "        401 => \"UNAUTHORIZED\",").unwrap();
    writeln!(out, "        403 => \"FORBIDDEN\",").unwrap();
    writeln!(out, "        404 => \"NOT_FOUND\",").unwrap();
    writeln!(out, "        409 => \"CONFLICT\",").unwrap();
    writeln!(out, "        413 => \"PAYLOAD_TOO_LARGE\",").unwrap();
    writeln!(out, "        422 => \"VALIDATION_ERROR\",").unwrap();
    writeln!(out, "        429 => \"RATE_LIMITED\",").unwrap();
    writeln!(out, "        _ => \"INTERNAL_ERROR\",").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    let body = serde_json::json!({{").unwrap();
    writeln!(out, "        \"error\": message,").unwrap();
    writeln!(out, "        \"error_type\": error_type,").unwrap();
    writeln!(out, "        \"code\": status.as_u16(),").unwrap();
    writeln!(out, "    }});").unwrap();
    writeln!(out, "    (status, Json(body)).into_response()").unwrap();
    writeln!(out, "}}").unwrap();
}

fn generate_row_json_fns(out: &mut String, dialects: &HashSet<Dialect>) {
    if dialects.contains(&Dialect::Postgres) {
        writeln!(
            out,
            "fn row_to_json(row: &sqlx::postgres::PgRow) -> serde_json::Value {{"
        )
        .unwrap();
        writeln!(out, "    use sqlx::{{Row, Column}};").unwrap();
        writeln!(out, "    let mut map = serde_json::Map::new();").unwrap();
        writeln!(out, "    for col in row.columns() {{").unwrap();
        writeln!(out, "        let name = col.name();").unwrap();
        writeln!(out, "        let val: serde_json::Value = row.try_get::<bool, _>(name).map(|v| serde_json::json!(v))").unwrap();
        writeln!(
            out,
            "            .or_else(|_| row.try_get::<i32, _>(name).map(|v| serde_json::json!(v)))"
        )
        .unwrap();
        writeln!(
            out,
            "            .or_else(|_| row.try_get::<i64, _>(name).map(|v| serde_json::json!(v)))"
        )
        .unwrap();
        writeln!(
            out,
            "            .or_else(|_| row.try_get::<f64, _>(name).map(|v| serde_json::json!(v)))"
        )
        .unwrap();
        writeln!(out, "            .or_else(|_| row.try_get::<rust_decimal::Decimal, _>(name).map(|v| serde_json::json!(v.to_string())))").unwrap();
        writeln!(out, "            .or_else(|_| row.try_get::<uuid::Uuid, _>(name).map(|v| serde_json::json!(v.to_string())))").unwrap();
        writeln!(out, "            .or_else(|_| row.try_get::<chrono::NaiveDate, _>(name).map(|v| serde_json::json!(v.to_string())))").unwrap();
        writeln!(out, "            .or_else(|_| row.try_get::<chrono::DateTime<chrono::Utc>, _>(name).map(|v| serde_json::json!(v.to_rfc3339())))").unwrap();
        writeln!(
            out,
            "            .or_else(|_| row.try_get::<serde_json::Value, _>(name).map(|v| v))"
        )
        .unwrap();
        writeln!(
            out,
            "            .or_else(|_| row.try_get::<String, _>(name).map(|v| serde_json::json!(v)))"
        )
        .unwrap();
        writeln!(out, "            .unwrap_or(serde_json::Value::Null);").unwrap();
        writeln!(out, "        map.insert(name.to_string(), val);").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    serde_json::Value::Object(map)").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
    if dialects.contains(&Dialect::Mysql) && !dialects.contains(&Dialect::Postgres) {
        writeln!(
            out,
            "fn row_to_json(row: &sqlx::mysql::MySqlRow) -> serde_json::Value {{"
        )
        .unwrap();
        writeln!(out, "    use sqlx::{{Row, Column, TypeInfo}};").unwrap();
        writeln!(out, "    let mut map = serde_json::Map::new();").unwrap();
        writeln!(out, "    for col in row.columns() {{").unwrap();
        writeln!(out, "        let name = col.name();").unwrap();
        writeln!(out, "        let tn = col.type_info().name();").unwrap();
        writeln!(
            out,
            "        let val: serde_json::Value = if tn == \"BOOLEAN\" || tn == \"TINYINT(1)\" {{"
        )
        .unwrap();
        writeln!(out, "            row.try_get::<bool, _>(name).map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null)").unwrap();
        writeln!(out, "        }} else {{").unwrap();
        writeln!(
            out,
            "            row.try_get::<i32, _>(name).map(|v| serde_json::json!(v))"
        )
        .unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<i64, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<f64, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<String, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .unwrap_or(serde_json::Value::Null)").unwrap();
        writeln!(out, "        }};").unwrap();
        writeln!(out, "        map.insert(name.to_string(), val);").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    serde_json::Value::Object(map)").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
    if dialects.contains(&Dialect::Sqlite)
        && !dialects.contains(&Dialect::Postgres)
        && !dialects.contains(&Dialect::Mysql)
    {
        writeln!(
            out,
            "fn row_to_json(row: &sqlx::sqlite::SqliteRow) -> serde_json::Value {{"
        )
        .unwrap();
        writeln!(out, "    use sqlx::{{Row, Column, TypeInfo}};").unwrap();
        writeln!(out, "    let mut map = serde_json::Map::new();").unwrap();
        writeln!(out, "    for col in row.columns() {{").unwrap();
        writeln!(out, "        let name = col.name();").unwrap();
        writeln!(out, "        let tn = col.type_info().name();").unwrap();
        writeln!(
            out,
            "        let val: serde_json::Value = if tn == \"BOOLEAN\" {{"
        )
        .unwrap();
        writeln!(out, "            row.try_get::<bool, _>(name).map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null)").unwrap();
        writeln!(out, "        }} else {{").unwrap();
        writeln!(
            out,
            "            row.try_get::<i32, _>(name).map(|v| serde_json::json!(v))"
        )
        .unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<i64, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<f64, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .or_else(|_| row.try_get::<String, _>(name).map(|v| serde_json::json!(v)))").unwrap();
        writeln!(out, "                .unwrap_or(serde_json::Value::Null)").unwrap();
        writeln!(out, "        }};").unwrap();
        writeln!(out, "        map.insert(name.to_string(), val);").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "    serde_json::Value::Object(map)").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }
}

fn generate_request_id_middleware(out: &mut String) {
    writeln!(out, "async fn request_id_middleware(").unwrap();
    writeln!(out, "    mut req: Request<axum::body::Body>,").unwrap();
    writeln!(out, "    next: middleware::Next,").unwrap();
    writeln!(out, ") -> axum::response::Response {{").unwrap();
    writeln!(out, "    let request_id = req.headers()").unwrap();
    writeln!(out, "        .get(\"x-request-id\")").unwrap();
    writeln!(out, "        .and_then(|v| v.to_str().ok())").unwrap();
    writeln!(out, "        .map(|s| s.to_string())").unwrap();
    writeln!(
        out,
        "        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());"
    )
    .unwrap();
    writeln!(out, "    req.extensions_mut().insert(request_id.clone());").unwrap();
    writeln!(out, "    let span = tracing::info_span!(\"request\", request_id = %request_id, method = %req.method(), uri = %req.uri());").unwrap();
    writeln!(out, "    async move {{").unwrap();
    writeln!(out, "        tracing::info!(\"request started\");").unwrap();
    writeln!(out, "        let start = std::time::Instant::now();").unwrap();
    writeln!(out, "        let mut response = next.run(req).await;").unwrap();
    writeln!(out, "        let duration = start.elapsed();").unwrap();
    writeln!(
        out,
        "        response.headers_mut().insert(\"x-request-id\", request_id.parse().unwrap());"
    )
    .unwrap();
    writeln!(out, "        tracing::info!(status = %response.status(), duration_ms = %duration.as_millis(), \"request completed\");").unwrap();
    writeln!(out, "        response").unwrap();
    writeln!(out, "    }}.instrument(span).await").unwrap();
    writeln!(out, "}}").unwrap();
}

fn generate_db_try_macro(out: &mut String) {
    writeln!(out, "macro_rules! db_try {{").unwrap();
    writeln!(out, "    ($expr:expr) => {{").unwrap();
    writeln!(out, "        match $expr {{").unwrap();
    writeln!(out, "            Ok(v) => v,").unwrap();
    writeln!(out, "            Err(e) => {{").unwrap();
    writeln!(
        out,
        "                tracing::error!(\"database error: {{}}\", e);"
    )
    .unwrap();
    writeln!(out, "                return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"internal server error\");").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    ($expr:expr, $label:lifetime) => {{").unwrap();
    writeln!(out, "        match $expr {{").unwrap();
    writeln!(out, "            Ok(v) => v,").unwrap();
    writeln!(out, "            Err(e) => {{").unwrap();
    writeln!(
        out,
        "                tracing::error!(\"database error: {{}}\", e);"
    )
    .unwrap();
    writeln!(
        out,
        "                break $label Some(format!(\"database error: {{}}\", e));"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "}}").unwrap();
}

fn rust_type_for(ty: &TypeExpr) -> &'static str {
    match ty {
        TypeExpr::Uuid => "uuid::Uuid",
        TypeExpr::String(_) | TypeExpr::Text => "String",
        TypeExpr::Int { .. } => "i64",
        TypeExpr::Decimal { .. } => "rust_decimal::Decimal",
        TypeExpr::Bool => "bool",
        TypeExpr::Date => "chrono::NaiveDate",
        TypeExpr::Timestamp => "chrono::DateTime<chrono::Utc>",
        TypeExpr::Json => "serde_json::Value",
        TypeExpr::Enum(_) => "String",
        TypeExpr::Maybe(inner) => rust_type_for(inner),
        TypeExpr::List(_) => "Vec<serde_json::Value>",
        TypeExpr::Map(_, _) => "std::collections::HashMap<String, serde_json::Value>",
        TypeExpr::Ref { .. } => "uuid::Uuid",
        TypeExpr::Blob => "Vec<u8>",
    }
}

fn generate_to_decimal(out: &mut String) {
    writeln!(
        out,
        "fn _to_decimal(v: &serde_json::Value) -> rust_decimal::Decimal {{"
    )
    .unwrap();
    writeln!(out, "    use std::str::FromStr;").unwrap();
    writeln!(
        out,
        "    v.as_str().and_then(|s| rust_decimal::Decimal::from_str(s).ok())"
    )
    .unwrap();
    writeln!(
        out,
        "        .or_else(|| v.as_i64().map(rust_decimal::Decimal::from))"
    )
    .unwrap();
    writeln!(
        out,
        "        .or_else(|| v.as_f64().and_then(|f| rust_decimal::Decimal::try_from(f).ok()))"
    )
    .unwrap();
    writeln!(out, "        .unwrap_or_default()").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
}

fn generate_upload_helpers(out: &mut String, has_s3: bool) {
    writeln!(
        out,
        "type AxisUploadParts = (Vec<u8>, Option<String>, Option<String>);"
    )
    .unwrap();
    writeln!(
        out,
        "fn axis_upload_parts(value: &serde_json::Value) -> Result<AxisUploadParts, String> {{"
    )
    .unwrap();
    writeln!(out, "    let decode_array = |values: &[serde_json::Value]| values.iter().map(|value| value.as_u64().and_then(|byte| u8::try_from(byte).ok()).ok_or_else(|| \"upload byte arrays must contain integers from 0 to 255\".to_string())).collect::<Result<Vec<_>, _>>();").unwrap();
    writeln!(out, "    match value {{").unwrap();
    writeln!(
        out,
        "        serde_json::Value::String(value) => Ok((value.as_bytes().to_vec(), None, None)),"
    )
    .unwrap();
    writeln!(out, "        serde_json::Value::Array(values) => decode_array(values).map(|bytes| (bytes, None, None)),").unwrap();
    writeln!(out, "        serde_json::Value::Object(object) => {{").unwrap();
    writeln!(out, "            let values = object.get(\"bytes\").and_then(serde_json::Value::as_array).ok_or_else(|| \"upload objects require a bytes array\".to_string())?;").unwrap();
    writeln!(out, "            let filename = object.get(\"filename\").and_then(serde_json::Value::as_str).map(str::to_owned);").unwrap();
    writeln!(out, "            let content_type = object.get(\"content_type\").and_then(serde_json::Value::as_str).map(str::to_ascii_lowercase);").unwrap();
    writeln!(
        out,
        "            decode_array(values).map(|bytes| (bytes, filename, content_type))"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        _ => Err(\"upload: expected multipart file data, a byte array, or a string\".to_string()),").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "fn axis_safe_upload_extension(bytes: &[u8], filename: Option<&str>, content_type: Option<&str>, allowed_types: &[&str]) -> Result<String, String> {{").unwrap();
    writeln!(out, "    let detected = infer::get(bytes);").unwrap();
    writeln!(
        out,
        "    let detected_mime = detected.map(|kind| kind.mime_type().to_ascii_lowercase());"
    )
    .unwrap();
    writeln!(
        out,
        "    let detected_extension = detected.map(|kind| kind.extension().to_ascii_lowercase());"
    )
    .unwrap();
    writeln!(out, "    let supplied_extension = filename.and_then(|name| std::path::Path::new(name).extension()).and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase).filter(|extension| !extension.is_empty() && extension.len() <= 16 && extension.chars().all(|character| character.is_ascii_alphanumeric()));").unwrap();
    writeln!(
        out,
        "    let content_type = content_type.map(str::to_ascii_lowercase);"
    )
    .unwrap();
    writeln!(
        out,
        "    if !allowed_types.is_empty() && !allowed_types.iter().any(|allowed| {{"
    )
    .unwrap();
    writeln!(
        out,
        "        let allowed = allowed.trim_start_matches('.').to_ascii_lowercase();"
    )
    .unwrap();
    writeln!(out, "        detected_mime.as_deref() == Some(allowed.as_str()) || detected_extension.as_deref() == Some(allowed.as_str()) || (detected.is_none() && (content_type.as_deref() == Some(allowed.as_str()) || supplied_extension.as_deref() == Some(allowed.as_str())))").unwrap();
    writeln!(
        out,
        "    }}) {{ return Err(\"file type is not allowed by the storage policy\".to_string()); }}"
    )
    .unwrap();
    writeln!(
        out,
        "    Ok(detected_extension.or(supplied_extension).unwrap_or_else(|| \"bin\".into()))"
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    if has_s3 {
        writeln!(
            out,
            "fn axis_s3_public_url(storage_name: &str, bucket: &str, object_key: &str) -> String {{"
        )
        .unwrap();
        writeln!(out, "    let storage_key = format!(\"AXIS_STORAGE_{{}}_PUBLIC_BASE_URL\", storage_name.chars().map(|character| if character.is_ascii_alphanumeric() {{ character.to_ascii_uppercase() }} else {{ '_' }}).collect::<String>());").unwrap();
        writeln!(out, "    let base = std::env::var(storage_key).or_else(|_| std::env::var(\"AXIS_S3_PUBLIC_BASE_URL\")).unwrap_or_else(|_| format!(\"https://{{bucket}}.s3.amazonaws.com\"));").unwrap();
        writeln!(
            out,
            "    format!(\"{{}}/{{}}\", base.trim_end_matches('/'), object_key)"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
    }
}

fn lower_expr(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            LiteralValue::Int(n) => format!("serde_json::json!({n})"),
            LiteralValue::Decimal(d) => format!("serde_json::json!({d})"),
            LiteralValue::String(s) => format!("serde_json::json!(\"{}\")", s.replace('"', "\\\"")),
            LiteralValue::Bool(b) => format!("serde_json::json!({b})"),
            LiteralValue::Ident(id) => format!("serde_json::json!(\"{id}\")"),
            LiteralValue::Now => "serde_json::json!(chrono::Utc::now().to_rfc3339())".into(),
            LiteralValue::None => "serde_json::Value::Null".into(),
        },
        Expr::DotPath(dp) => lower_dot_path(dp),
        Expr::Binary { op, left, right } => {
            let l = lower_expr(left);
            let r = lower_expr(right);
            match op {
                BinaryOp::Add => format!(
                    "serde_json::json!((_to_decimal(&{l}) + _to_decimal(&{r})).to_string())"
                ),
                BinaryOp::Sub => format!(
                    "serde_json::json!((_to_decimal(&{l}) - _to_decimal(&{r})).to_string())"
                ),
                BinaryOp::Mul => format!(
                    "serde_json::json!((_to_decimal(&{l}) * _to_decimal(&{r})).to_string())"
                ),
                BinaryOp::Div => format!(
                    "serde_json::json!((_to_decimal(&{l}) / _to_decimal(&{r})).to_string())"
                ),
                BinaryOp::Mod => format!(
                    "serde_json::json!(({l}).as_i64().unwrap_or(0) % ({r}).as_i64().unwrap_or(1))"
                ),
                BinaryOp::And => format!(
                    "serde_json::json!(({l}).as_bool().unwrap_or(false) && ({r}).as_bool().unwrap_or(false))"
                ),
                BinaryOp::Or => format!(
                    "serde_json::json!(({l}).as_bool().unwrap_or(false) || ({r}).as_bool().unwrap_or(false))"
                ),
                BinaryOp::Eq => format!("serde_json::json!(({l}) == ({r}))"),
                BinaryOp::Neq => format!("serde_json::json!(({l}) != ({r}))"),
                BinaryOp::Gt => format!(
                    "serde_json::json!(({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") > ({r}).as_str().unwrap_or(\"\"), |(a, b)| a > b))"
                ),
                BinaryOp::Gte => format!(
                    "serde_json::json!(({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") >= ({r}).as_str().unwrap_or(\"\"), |(a, b)| a >= b))"
                ),
                BinaryOp::Lt => format!(
                    "serde_json::json!(({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") < ({r}).as_str().unwrap_or(\"\"), |(a, b)| a < b))"
                ),
                BinaryOp::Lte => format!(
                    "serde_json::json!(({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") <= ({r}).as_str().unwrap_or(\"\"), |(a, b)| a <= b))"
                ),
                BinaryOp::Concat => format!(
                    "serde_json::json!(format!(\"{{}}{{}}\", ({l}).as_str().unwrap_or(\"\"), ({r}).as_str().unwrap_or(\"\")))"
                ),
                BinaryOp::StartsWith => format!(
                    "serde_json::json!(({l}).as_str().unwrap_or(\"\").starts_with(({r}).as_str().unwrap_or(\"\")))"
                ),
                BinaryOp::EndsWith => format!(
                    "serde_json::json!(({l}).as_str().unwrap_or(\"\").ends_with(({r}).as_str().unwrap_or(\"\")))"
                ),
                BinaryOp::Contains => format!(
                    "serde_json::json!(({l}).as_str().unwrap_or(\"\").contains(({r}).as_str().unwrap_or(\"\")))"
                ),
                BinaryOp::DaysBetween => format!(
                    "{{ let _av = {l}; let _bv = {r}; let _as = _av.as_str().unwrap_or(\"\"); let _bs = _bv.as_str().unwrap_or(\"\"); let _a = chrono::DateTime::parse_from_rfc3339(_as).map(|d| d.date_naive()).or_else(|_| chrono::NaiveDate::parse_from_str(_as, \"%Y-%m-%d\")); let _b = chrono::DateTime::parse_from_rfc3339(_bs).map(|d| d.date_naive()).or_else(|_| chrono::NaiveDate::parse_from_str(_bs, \"%Y-%m-%d\")); serde_json::json!(_a.and_then(|a| _b.map(|b| (b - a).num_days())).unwrap_or(0)) }}"
                ),
                BinaryOp::HoursBetween => format!(
                    "{{ let _av = {l}; let _bv = {r}; let _a = chrono::DateTime::parse_from_rfc3339(_av.as_str().unwrap_or(\"\")).or_else(|_| chrono::NaiveDate::parse_from_str(_av.as_str().unwrap_or(\"\"), \"%Y-%m-%d\").map(|d| d.and_hms_opt(0,0,0).unwrap().and_utc().fixed_offset())); let _b = chrono::DateTime::parse_from_rfc3339(_bv.as_str().unwrap_or(\"\")).or_else(|_| chrono::NaiveDate::parse_from_str(_bv.as_str().unwrap_or(\"\"), \"%Y-%m-%d\").map(|d| d.and_hms_opt(0,0,0).unwrap().and_utc().fixed_offset())); serde_json::json!(_a.and_then(|a| _b.map(|b| (b - a).num_hours())).unwrap_or(0)) }}"
                ),
                BinaryOp::MinutesBetween => format!(
                    "{{ let _av = {l}; let _bv = {r}; let _a = chrono::DateTime::parse_from_rfc3339(_av.as_str().unwrap_or(\"\")).or_else(|_| chrono::NaiveDate::parse_from_str(_av.as_str().unwrap_or(\"\"), \"%Y-%m-%d\").map(|d| d.and_hms_opt(0,0,0).unwrap().and_utc().fixed_offset())); let _b = chrono::DateTime::parse_from_rfc3339(_bv.as_str().unwrap_or(\"\")).or_else(|_| chrono::NaiveDate::parse_from_str(_bv.as_str().unwrap_or(\"\"), \"%Y-%m-%d\").map(|d| d.and_hms_opt(0,0,0).unwrap().and_utc().fixed_offset())); serde_json::json!(_a.and_then(|a| _b.map(|b| (b - a).num_minutes())).unwrap_or(0)) }}"
                ),
                BinaryOp::Round => format!(
                    "serde_json::json!(_to_decimal(&{l}).round_dp(({r}).as_u64().unwrap_or(0) as u32).to_string())"
                ),
                BinaryOp::Coalesce => {
                    format!("if ({l}).is_null() {{ {r} }} else {{ ({l}).clone() }}")
                }
                BinaryOp::FormatDate => format!(
                    "serde_json::json!(chrono::DateTime::parse_from_rfc3339(({l}).as_str().unwrap_or(\"\")).map(|d| d.format(({r}).as_str().unwrap_or(\"%Y-%m-%d\")).to_string()).unwrap_or_default())"
                ),
            }
        }
        Expr::Unary { op, operand } => {
            let o = lower_expr(operand);
            match op {
                UnaryOp::Not => format!("serde_json::json!(!({o}).as_bool().unwrap_or(false))"),
                UnaryOp::Empty => format!(
                    "serde_json::json!(({o}).is_null() || ({o}).as_str().map_or(false, |s| s.is_empty()))"
                ),
                UnaryOp::Exists => format!("serde_json::json!(!({o}).is_null())"),
                UnaryOp::Lower => {
                    format!("serde_json::json!(({o}).as_str().unwrap_or(\"\").to_lowercase())")
                }
                UnaryOp::Upper => {
                    format!("serde_json::json!(({o}).as_str().unwrap_or(\"\").to_uppercase())")
                }
                UnaryOp::Trim => {
                    format!("serde_json::json!(({o}).as_str().unwrap_or(\"\").trim())")
                }
                UnaryOp::Abs => format!("serde_json::json!(_to_decimal(&{o}).abs().to_string())"),
                UnaryOp::Ceil => format!("serde_json::json!(_to_decimal(&{o}).ceil().to_string())"),
                UnaryOp::Floor => {
                    format!("serde_json::json!(_to_decimal(&{o}).floor().to_string())")
                }
                UnaryOp::Length => format!(
                    "serde_json::json!(({o}).as_str().map_or(({o}).as_array().map_or(0, |a| a.len()), |s| s.len()))"
                ),
                UnaryOp::First => format!(
                    "({o}).as_array().and_then(|a| a.first()).cloned().unwrap_or(serde_json::Value::Null)"
                ),
                UnaryOp::Last => format!(
                    "({o}).as_array().and_then(|a| a.last()).cloned().unwrap_or(serde_json::Value::Null)"
                ),
                UnaryOp::ToInt => format!(
                    "serde_json::json!(({o}).as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| ({o}).as_f64().map(|f| f as i64)).unwrap_or(0))"
                ),
                UnaryOp::ToDecimal => format!("serde_json::json!(_to_decimal(&{o}).to_string())"),
                UnaryOp::ToString => format!(
                    "serde_json::json!(({o}).as_str().map(|s| s.to_string()).unwrap_or_else(|| ({o}).to_string()))"
                ),
                UnaryOp::Count => {
                    format!("serde_json::json!(({o}).as_array().map_or(0, |a| a.len()))")
                }
            }
        }
        Expr::Ternary { op, a, b, c } => {
            let va = lower_expr(a);
            let vb = lower_expr(b);
            let vc = lower_expr(c);
            match op {
                TernaryOp::Substring => format!(
                    "serde_json::json!(({va}).as_str().unwrap_or(\"\").chars().skip(({vb}).as_u64().unwrap_or(0) as usize).take(({vc}).as_u64().unwrap_or(0) as usize).collect::<String>())"
                ),
                TernaryOp::Between => format!(
                    "serde_json::json!(_to_decimal(&{va}) >= _to_decimal(&{vb}) && _to_decimal(&{va}) <= _to_decimal(&{vc}))"
                ),
            }
        }
        Expr::If { cond, then, else_ } => {
            let c = lower_expr_bool(cond);
            let t = lower_expr(then);
            let e = lower_expr(else_);
            format!("if {c} {{ {t} }} else {{ {e} }}")
        }
        Expr::Aggregate { op, source, field } => {
            let s = lower_expr(source);
            lower_aggregate(op, &s, field.as_deref())
        }
        Expr::Coalesce { value, default } => {
            let v = lower_expr(value);
            let d = lower_expr(default);
            format!("if ({v}).is_null() {{ {d} }} else {{ ({v}).clone() }}")
        }
        Expr::NowOffset {
            direction,
            amount,
            unit,
        } => {
            let amt = lower_expr(amount);
            let sign = match direction {
                OffsetDirection::Plus => "+",
                OffsetDirection::Minus => "-",
            };
            let dur = match unit {
                TimeUnit::Seconds => {
                    format!("chrono::Duration::seconds(({amt}).as_i64().unwrap_or(0))")
                }
                TimeUnit::Minutes => {
                    format!("chrono::Duration::minutes(({amt}).as_i64().unwrap_or(0))")
                }
                TimeUnit::Hours => {
                    format!("chrono::Duration::hours(({amt}).as_i64().unwrap_or(0))")
                }
                TimeUnit::Days => format!("chrono::Duration::days(({amt}).as_i64().unwrap_or(0))"),
                TimeUnit::Weeks => {
                    format!("chrono::Duration::weeks(({amt}).as_i64().unwrap_or(0))")
                }
                TimeUnit::Months => {
                    format!("chrono::Duration::days(({amt}).as_i64().unwrap_or(0) * 30)")
                }
                TimeUnit::Years => {
                    format!("chrono::Duration::days(({amt}).as_i64().unwrap_or(0) * 365)")
                }
            };
            format!("serde_json::json!((chrono::Utc::now() {sign} {dur}).to_rfc3339())")
        }
        Expr::Cached { expr, .. } => lower_expr(expr),
        Expr::MapExpr { source, fields } => {
            let s = lower_expr(source);
            let picks: Vec<String> = fields
                .iter()
                .map(|f| format!("(\"{f}\".to_string(), item[\"{f}\"].clone())"))
                .collect();
            format!(
                "serde_json::json!(({s}).as_array().map(|arr| arr.iter().map(|item| serde_json::Value::Object(vec![{}].into_iter().collect())).collect::<Vec<_>>()).unwrap_or_default())",
                picks.join(", ")
            )
        }
        Expr::FilterExpr { source, condition } => {
            let s = lower_expr(source);
            let c = lower_expr_bool(condition);
            format!(
                "serde_json::json!(({s}).as_array().map(|arr| arr.iter().filter(|_| {c}).cloned().collect::<Vec<_>>()).unwrap_or_default())"
            )
        }
        Expr::ReduceExpr { op, source, field } => {
            let s = lower_expr(source);
            lower_aggregate(op, &s, Some(field.as_str()))
        }
        Expr::SplitExpr { value, delimiter } => {
            let v = lower_expr(value);
            let d = lower_expr(delimiter);
            format!(
                "serde_json::json!(({v}).as_str().unwrap_or(\"\").split(({d}).as_str().unwrap_or(\",\")).collect::<Vec<&str>>())"
            )
        }
        Expr::ReplaceExpr { value, from, to } => {
            let v = lower_expr(value);
            let f = lower_expr(from);
            let t = lower_expr(to);
            format!(
                "serde_json::json!(({v}).as_str().unwrap_or(\"\").replace(({f}).as_str().unwrap_or(\"\"), ({t}).as_str().unwrap_or(\"\")))"
            )
        }
        Expr::FormatExpr { template, args } => {
            if args.is_empty() {
                return format!("serde_json::json!(\"{}\")", template.replace('"', "\\\""));
            }
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    let e = lower_expr(a);
                    format!(
                        "({e}).as_str().map(|s| s.to_string()).unwrap_or_else(|| ({e}).to_string())"
                    )
                })
                .collect();
            let mut result = format!(
                "{{ let mut _t = \"{}\".to_string(); ",
                template.replace('"', "\\\"")
            );
            for a in &arg_strs {
                result.push_str(&format!("_t = _t.replacen(\"{{}}\", &{a}, 1); "));
            }
            result.push_str("serde_json::json!(_t) }");
            result
        }
        Expr::Render { template, vars } => {
            let mut result = format!(
                "{{ let mut _t = \"{}\".to_string(); ",
                template.replace('"', "\\\"")
            );
            for (k, v) in vars {
                let e = lower_expr(v);
                let val = format!(
                    "({e}).as_str().map(|s| s.to_string()).unwrap_or_else(|| ({e}).to_string())"
                );
                result.push_str(&format!("_t = _t.replace(\"{{{{{k}}}}}\", &{val}); "));
            }
            result.push_str("serde_json::json!(_t) }");
            result
        }
        Expr::Translate { key, vars } => {
            let mut result = format!(
                "{{ let mut _t = \"{}\".to_string(); ",
                key.replace('"', "\\\"")
            );
            for (k, v) in vars {
                let e = lower_expr(v);
                let val = format!(
                    "({e}).as_str().map(|s| s.to_string()).unwrap_or_else(|| ({e}).to_string())"
                );
                result.push_str(&format!("_t = _t.replace(\"{{{{{k}}}}}\", &{val}); "));
            }
            result.push_str("serde_json::json!(_t) }");
            result
        }
        Expr::FuncCall { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(lower_expr).collect();
            format!("func_{name}({})", arg_strs.join(", "))
        }
        Expr::Fetch { .. } | Expr::Query { .. } | Expr::Call { .. } | Expr::WasmCall { .. } => {
            "serde_json::Value::Null".into()
        }
    }
}

fn is_non_numeric_expr(expr: &Expr) -> bool {
    if let Expr::DotPath(dp) = expr {
        let segs = &dp.segments;
        if segs.len() == 2 && segs[0] == "body" {
            return BODY_FIELDS.with(|bf| {
                bf.borrow().get(&segs[1]).is_some_and(|ty| {
                    matches!(
                        ty,
                        TypeExpr::Date | TypeExpr::Timestamp | TypeExpr::String(_) | TypeExpr::Text
                    )
                })
            });
        }
    }
    false
}

fn lower_expr_bool(expr: &Expr) -> String {
    match expr {
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Eq => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                format!("({l}) == ({r})")
            }
            BinaryOp::Neq => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                format!("({l}) != ({r})")
            }
            BinaryOp::Gt | BinaryOp::Gte | BinaryOp::Lt | BinaryOp::Lte => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                let op_str = match op {
                    BinaryOp::Gt => ">",
                    BinaryOp::Gte => ">=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Lte => "<=",
                    _ => unreachable!(),
                };
                if is_non_numeric_expr(left) || is_non_numeric_expr(right) {
                    format!(
                        "({l}).as_str().unwrap_or(\"\") {op_str} ({r}).as_str().unwrap_or(\"\")"
                    )
                } else {
                    format!(
                        "({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") {op_str} ({r}).as_str().unwrap_or(\"\"), |(a, b)| a {op_str} b)"
                    )
                }
            }
            BinaryOp::And => {
                let l = lower_expr_bool(left);
                let r = lower_expr_bool(right);
                format!("({l}) && ({r})")
            }
            BinaryOp::Or => {
                let l = lower_expr_bool(left);
                let r = lower_expr_bool(right);
                format!("({l}) || ({r})")
            }
            BinaryOp::Contains => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                format!("({l}).as_str().unwrap_or(\"\").contains(({r}).as_str().unwrap_or(\"\"))")
            }
            BinaryOp::StartsWith => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                format!(
                    "({l}).as_str().unwrap_or(\"\").starts_with(({r}).as_str().unwrap_or(\"\"))"
                )
            }
            BinaryOp::EndsWith => {
                let l = lower_expr(left);
                let r = lower_expr(right);
                format!("({l}).as_str().unwrap_or(\"\").ends_with(({r}).as_str().unwrap_or(\"\"))")
            }
            _ => format!("({}).as_bool().unwrap_or(false)", lower_expr(expr)),
        },
        Expr::Unary { op, operand } => match op {
            UnaryOp::Not => format!("!({})", lower_expr_bool(operand)),
            UnaryOp::Empty => {
                let o = lower_expr(operand);
                format!(
                    "(({o}).is_null() || ({o}).as_str().map_or(false, |s| s.is_empty()) || ({o}).as_array().map_or(false, |a| a.is_empty()))"
                )
            }
            UnaryOp::Exists => {
                let o = lower_expr(operand);
                format!("!({o}).is_null()")
            }
            _ => format!("({}).as_bool().unwrap_or(false)", lower_expr(expr)),
        },
        Expr::Literal(LiteralValue::Bool(b)) => format!("{b}"),
        _ => format!("({}).as_bool().unwrap_or(false)", lower_expr(expr)),
    }
}

fn lower_dot_path(dp: &DotPath) -> String {
    let segs = &dp.segments;
    if segs.is_empty() {
        return "serde_json::Value::Null".into();
    }
    if segs.len() == 1 {
        let is_var = FLOW_SCOPE.with(|s| s.borrow().contains(&segs[0]));
        return if is_var {
            format!("{}.clone()", segs[0])
        } else {
            format!("serde_json::json!(\"{}\")", segs[0])
        };
    }
    let root = &segs[0];
    match root.as_str() {
        "body" | "path" | "query" => {
            let field_access = segs.join(".");
            format!("serde_json::to_value(&{field_access}).unwrap_or(serde_json::Value::Null)")
        }
        "header" => {
            let name = segs[1..].join("-").replace('_', "-");
            format!(
                "headers.get(\"{name}\").and_then(|value| value.to_str().ok()).map(|value| serde_json::json!(value)).unwrap_or(serde_json::Value::Null)"
            )
        }
        "auth" => {
            if segs.len() >= 2 {
                match segs[1].as_str() {
                    "sub" | "role" | "exp" => {
                        let field_access = segs.join(".");
                        format!(
                            "serde_json::to_value(&{field_access}).unwrap_or(serde_json::Value::Null)"
                        )
                    }
                    field => format!(
                        "auth.extra.get(\"{field}\").cloned().unwrap_or(serde_json::Value::Null)"
                    ),
                }
            } else {
                "serde_json::to_value(&auth).unwrap_or(serde_json::Value::Null)".into()
            }
        }
        _ => {
            let mut access = root.clone();
            for seg in &segs[1..] {
                access = format!("{access}[\"{seg}\"]");
            }
            format!("{access}.clone()")
        }
    }
}

fn lower_aggregate(op: &AggregateOp, src: &str, field: Option<&str>) -> String {
    let accessor = match field {
        Some(f) => format!("_to_decimal(&item[\"{f}\"])"),
        None => "_to_decimal(item)".into(),
    };
    match op {
        AggregateOp::Count => {
            format!("serde_json::json!(({src}).as_array().map_or(0, |a| a.len()))")
        }
        AggregateOp::Sum => format!(
            "serde_json::json!(({src}).as_array().map(|a| a.iter().map(|item| {accessor}).fold(rust_decimal::Decimal::ZERO, |a, b| a + b).to_string()).unwrap_or_else(|| \"0\".into()))"
        ),
        AggregateOp::Avg => format!(
            "serde_json::json!(({src}).as_array().map(|a| {{ let s = a.iter().map(|item| {accessor}).fold(rust_decimal::Decimal::ZERO, |a, b| a + b); if a.is_empty() {{ \"0\".into() }} else {{ (s / rust_decimal::Decimal::from(a.len() as i64)).to_string() }} }}).unwrap_or_else(|| \"0\".into()))"
        ),
        AggregateOp::Min => format!(
            "serde_json::json!(({src}).as_array().map(|a| a.iter().map(|item| {accessor}).fold(rust_decimal::Decimal::MAX, |a, b| a.min(b)).to_string()).unwrap_or_else(|| \"0\".into()))"
        ),
        AggregateOp::Max => format!(
            "serde_json::json!(({src}).as_array().map(|a| a.iter().map(|item| {accessor}).fold(rust_decimal::Decimal::MIN, |a, b| a.max(b)).to_string()).unwrap_or_else(|| \"0\".into()))"
        ),
        AggregateOp::First => format!(
            "({src}).as_array().and_then(|a| a.first()).cloned().unwrap_or(serde_json::Value::Null)"
        ),
        AggregateOp::Last => format!(
            "({src}).as_array().and_then(|a| a.last()).cloned().unwrap_or(serde_json::Value::Null)"
        ),
    }
}

fn lower_expr_sql_bind(expr: &Expr) -> String {
    match expr {
        Expr::DotPath(dp) => {
            let segs = &dp.segments;
            if segs.len() >= 2 {
                let root = &segs[0];
                match root.as_str() {
                    "body" | "path" => segs.join("."),
                    "auth" => match segs[1].as_str() {
                        "sub" => "auth.sub.clone()".into(),
                        "role" => "auth.role.clone().unwrap_or_default()".into(),
                        "exp" => "auth.exp as i64".into(),
                        f => format!(
                            "auth.extra.get(\"{f}\").and_then(|v| v.as_str()).unwrap_or(\"\").to_string()"
                        ),
                    },
                    "query" => {
                        let is_copy = segs.len() >= 2
                            && QUERY_PARAMS.with(|qp| {
                                qp.borrow().get(&segs[1]).is_some_and(|ty| {
                                    matches!(ty, TypeExpr::Int { .. } | TypeExpr::Bool)
                                })
                            });
                        if is_copy {
                            format!("{}.unwrap_or_default()", segs.join("."))
                        } else {
                            format!("{}.clone().unwrap_or_default()", segs.join("."))
                        }
                    }
                    _ => {
                        let mut access = root.to_string();
                        for seg in &segs[1..] {
                            access = format!("{access}[\"{seg}\"]");
                        }
                        format!("{access}.as_str().unwrap_or(\"\").to_string()")
                    }
                }
            } else {
                let is_var = FLOW_SCOPE.with(|s| s.borrow().contains(&segs[0]));
                if is_var {
                    format!(
                        "{0}.as_str().map(|s| s.to_string()).unwrap_or_else(|| {0}.to_string())",
                        segs[0]
                    )
                } else {
                    format!("\"{}\".to_string()", segs[0])
                }
            }
        }
        Expr::Literal(lit) => match lit {
            LiteralValue::Int(n) => format!("{n}_i64"),
            LiteralValue::Decimal(d) => d.clone(),
            LiteralValue::String(s) => format!("\"{}\".to_string()", s.replace('"', "\\\"")),
            LiteralValue::Bool(b) => format!("{b}"),
            LiteralValue::Now => "chrono::Utc::now()".into(),
            LiteralValue::None => "Option::<String>::None".into(),
            LiteralValue::Ident(id) => format!("\"{}\".to_string()", id),
        },
        _ => format!(
            "{{ let _v = {}; _v.as_str().map(|s| s.to_string()).unwrap_or_else(|| _v.to_string()) }}",
            lower_expr(expr)
        ),
    }
}

fn lookup_column_type(source: &str, column: &str) -> Option<TypeExpr> {
    SOURCE_SHAPES.with(|ss| {
        let map = ss.borrow();
        map.get(source).and_then(|fields| {
            fields
                .iter()
                .find(|(n, _)| n == column)
                .map(|(_, t)| t.clone())
        })
    })
}

fn lower_expr_sql_bind_typed(expr: &Expr, source: &str, column: &str) -> String {
    let ty = lookup_column_type(source, column);
    if ty.is_none() {
        return lower_expr_sql_bind(expr);
    }
    let ty = ty.unwrap();
    fn is_json_value_expr(expr: &Expr) -> bool {
        match expr {
            Expr::DotPath(dp) => {
                let segs = &dp.segments;
                if segs.len() == 1 {
                    FLOW_SCOPE.with(|s| s.borrow().contains(&segs[0]))
                } else {
                    let root = segs[0].as_str();
                    !matches!(root, "body" | "path" | "query" | "auth")
                }
            }
            Expr::Binary { .. } | Expr::Call { .. } => true,
            _ => false,
        }
    }
    if !is_json_value_expr(expr) {
        return lower_expr_sql_bind(expr);
    }
    let val_expr = lower_expr(expr);
    fn extract_typed(val: &str, ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Int { .. } => format!("({val}).as_i64().unwrap_or(0)"),
            TypeExpr::Decimal { .. } => format!(
                "{{ let _v = &{val}; _v.as_str().and_then(|s| s.parse::<rust_decimal::Decimal>().ok()).or_else(|| _v.as_f64().and_then(|f| rust_decimal::Decimal::try_from(f).ok())).unwrap_or_default() }}"
            ),
            TypeExpr::Bool => format!("({val}).as_bool().unwrap_or(false)"),
            TypeExpr::Uuid
            | TypeExpr::String(_)
            | TypeExpr::Text
            | TypeExpr::Date
            | TypeExpr::Timestamp => format!("({val}).as_str().unwrap_or(\"\").to_string()"),
            TypeExpr::Maybe(inner) => match inner.as_ref() {
                TypeExpr::Int { .. } => format!("({val}).as_i64()"),
                TypeExpr::Decimal { .. } => format!(
                    "{{ let _v = &{val}; _v.as_str().and_then(|s| s.parse::<rust_decimal::Decimal>().ok()).or_else(|| _v.as_f64().and_then(|f| rust_decimal::Decimal::try_from(f).ok())) }}"
                ),
                TypeExpr::Bool => format!("({val}).as_bool()"),
                _ => format!("({val}).as_str().map(|s| s.to_string())"),
            },
            _ => format!("({val}).as_str().unwrap_or(\"\").to_string()"),
        }
    }
    extract_typed(&val_expr, &ty)
}

fn is_non_numeric_path(path: &DotPath) -> bool {
    let segs = &path.segments;
    if segs.len() == 2 && segs[0] == "body" {
        return BODY_FIELDS.with(|bf| {
            bf.borrow().get(&segs[1]).is_some_and(|ty| {
                matches!(
                    ty,
                    TypeExpr::Date | TypeExpr::Timestamp | TypeExpr::String(_) | TypeExpr::Text
                )
            })
        });
    }
    false
}

fn lower_rule_check(path: &DotPath, op: &CompareOp, rhs: &Expr) -> String {
    let l = lower_dot_path(path);
    let r = lower_expr(rhs);
    let str_only = is_non_numeric_path(path) || is_non_numeric_expr(rhs);
    match op {
        CompareOp::Eq => format!("({l}) == ({r})"),
        CompareOp::Neq => format!("({l}) != ({r})"),
        CompareOp::Gt if str_only => {
            format!("({l}).as_str().unwrap_or(\"\") > ({r}).as_str().unwrap_or(\"\")")
        }
        CompareOp::Gte if str_only => {
            format!("({l}).as_str().unwrap_or(\"\") >= ({r}).as_str().unwrap_or(\"\")")
        }
        CompareOp::Lt if str_only => {
            format!("({l}).as_str().unwrap_or(\"\") < ({r}).as_str().unwrap_or(\"\")")
        }
        CompareOp::Lte if str_only => {
            format!("({l}).as_str().unwrap_or(\"\") <= ({r}).as_str().unwrap_or(\"\")")
        }
        CompareOp::Gt => format!(
            "({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") > ({r}).as_str().unwrap_or(\"\"), |(a, b)| a > b)"
        ),
        CompareOp::Gte => format!(
            "({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") >= ({r}).as_str().unwrap_or(\"\"), |(a, b)| a >= b)"
        ),
        CompareOp::Lt => format!(
            "({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") < ({r}).as_str().unwrap_or(\"\"), |(a, b)| a < b)"
        ),
        CompareOp::Lte => format!(
            "({l}).as_f64().zip(({r}).as_f64()).map_or_else(|| ({l}).as_str().unwrap_or(\"\") <= ({r}).as_str().unwrap_or(\"\"), |(a, b)| a <= b)"
        ),
        CompareOp::In => format!("({r}).as_array().map_or(false, |arr| arr.contains(&({l})))"),
    }
}

fn sql_compare_op(op: &CompareOp) -> &'static str {
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

fn flow_has_mutations(steps: &[FlowStep]) -> bool {
    steps.iter().any(|s| match s {
        FlowStep::Insert(_)
        | FlowStep::Upsert(_)
        | FlowStep::Update(_)
        | FlowStep::Delete(_)
        | FlowStep::Fanout(_) => true,
        FlowStep::Match(m) => {
            m.branches.iter().any(|b| flow_has_mutations(&b.steps))
                || m.default.as_ref().is_some_and(|d| flow_has_mutations(d))
        }
        FlowStep::Each(e) => flow_has_mutations(&e.steps),
        FlowStep::Try(t) => flow_has_mutations(&t.body) || flow_has_mutations(&t.recover),
        _ => false,
    })
}

fn collect_mutated_sources(steps: &[FlowStep], sources: &mut HashSet<String>) {
    for step in steps {
        match step {
            FlowStep::Insert(ins) => {
                sources.insert(ins.source.clone());
            }
            FlowStep::Upsert(upsert) => {
                sources.insert(upsert.source.clone());
            }
            FlowStep::Update(upd) => {
                sources.insert(upd.source.clone());
            }
            FlowStep::Delete(del) => {
                sources.insert(del.source.clone());
            }
            FlowStep::Fanout(fanout) => {
                sources.insert(fanout.insert.source.clone());
            }
            FlowStep::Match(m) => {
                for b in &m.branches {
                    collect_mutated_sources(&b.steps, sources);
                }
                if let Some(d) = &m.default {
                    collect_mutated_sources(d, sources);
                }
            }
            FlowStep::Each(e) => collect_mutated_sources(&e.steps, sources),
            FlowStep::Try(t) => {
                collect_mutated_sources(&t.body, sources);
                collect_mutated_sources(&t.recover, sources);
            }
            _ => {}
        }
    }
}

fn collect_all_sources(steps: &[FlowStep], sources: &mut HashSet<String>) {
    for step in steps {
        match step {
            FlowStep::Let(l) => match &l.expr {
                Expr::Fetch { source, .. } | Expr::Query { source, .. } => {
                    sources.insert(source.clone());
                }
                _ => {}
            },
            FlowStep::Insert(ins) => {
                sources.insert(ins.source.clone());
            }
            FlowStep::Upsert(upsert) => {
                sources.insert(upsert.source.clone());
            }
            FlowStep::Update(upd) => {
                sources.insert(upd.source.clone());
            }
            FlowStep::Delete(del) => {
                sources.insert(del.source.clone());
            }
            FlowStep::Fanout(fanout) => {
                sources.insert(fanout.insert.source.clone());
            }
            FlowStep::Guard(g) => {
                if let Expr::Unary {
                    op: UnaryOp::Empty,
                    operand,
                } = &g.expr
                {
                    if let Expr::Query { source, .. } = operand.as_ref() {
                        sources.insert(source.clone());
                    }
                }
            }
            FlowStep::Match(m) => {
                for b in &m.branches {
                    collect_all_sources(&b.steps, sources);
                }
                if let Some(d) = &m.default {
                    collect_all_sources(d, sources);
                }
            }
            FlowStep::Each(e) => collect_all_sources(&e.steps, sources),
            FlowStep::Try(t) => {
                collect_all_sources(&t.body, sources);
                collect_all_sources(&t.recover, sources);
            }
            _ => {}
        }
    }
}

fn emit_filter_bind(out: &mut String, f: &FilterClause, source: &str, pad: &String) {
    if is_optional_query_ref(&f.value) {
        if let Expr::DotPath(dp) = &f.value {
            let path = dp.segments.join(".");
            writeln!(out, "{pad}    .bind({path})").unwrap();
            return;
        }
    }
    let bind = lower_expr_sql_bind_typed(&f.value, source, &f.field);
    if matches!(f.op, FilterOp::In) {
        writeln!(out, "{pad}    .bind(vec![{bind}])").unwrap();
    } else {
        writeln!(out, "{pad}    .bind({bind})").unwrap();
    }
}

fn db_ref_for(source: &str) -> String {
    DB_REF_OVERRIDE.with(|value| {
        value
            .borrow()
            .clone()
            .unwrap_or_else(|| format!("&state.db_{source}"))
    })
}

fn generate_flow_steps(out: &mut String, steps: &[FlowStep], indent: usize, db_ref: &str) {
    for step in steps {
        generate_flow_step(out, step, indent, db_ref);
    }
}

fn generate_flow_step(out: &mut String, step: &FlowStep, indent: usize, db_ref: &str) {
    let pad = " ".repeat(indent);
    match step {
        FlowStep::Let(l) => generate_let_step(out, l, indent, db_ref),
        FlowStep::Set(s) => {
            let val = lower_expr(&s.expr);
            writeln!(out, "{pad}let {name} = {val};", name = s.name).unwrap();
        }
        FlowStep::Insert(ins) => {
            let dialect = source_dialect(&ins.source);
            let pool_ref = db_ref_for(&ins.source);
            let cols: Vec<&str> = ins.fields.iter().map(|(n, _)| n.as_str()).collect();
            let placeholders: Vec<String> = (1..=cols.len()).map(|i| dialect.ph(i)).collect();
            let returning = if ins.binding.is_some() {
                dialect.returning_star()
            } else {
                ""
            };
            writeln!(out, "{pad}let _ins = db_try!(sqlx::query(\"INSERT INTO {src} ({cols}) VALUES ({vals}){returning}\")",
                src = ins.source, cols = cols.join(", "), vals = placeholders.join(", ")).unwrap();
            for (col, expr) in &ins.fields {
                let bind = lower_expr_sql_bind_typed(expr, &ins.source, col);
                writeln!(out, "{pad}    .bind({bind})").unwrap();
            }
            if let Some(binding) = &ins.binding {
                if dialect == Dialect::Mysql {
                    writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
                    writeln!(out, "{pad}let {binding} = serde_json::json!({{\"id\": uuid::Uuid::new_v4().to_string()}});").unwrap();
                } else {
                    writeln!(out, "{pad}    .fetch_one({pool_ref}).await);").unwrap();
                    writeln!(out, "{pad}let {binding} = row_to_json(&_ins);").unwrap();
                }
            } else {
                writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
            }
            writeln!(
                out,
                "{pad}tracing::info!(source = \"{src}\", op = \"INSERT\", \"mutation executed\");",
                src = ins.source
            )
            .unwrap();
        }
        FlowStep::Upsert(upsert) => {
            let dialect = source_dialect(&upsert.source);
            let pool_ref = db_ref_for(&upsert.source);
            let mut fields: Vec<(&str, &Expr)> = upsert
                .keys
                .iter()
                .map(|(name, expr)| (name.as_str(), expr))
                .collect();
            fields.extend(
                upsert
                    .sets
                    .iter()
                    .map(|set| (set.field.as_str(), &set.value)),
            );
            let cols: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
            let placeholders: Vec<String> = (1..=cols.len()).map(|i| dialect.ph(i)).collect();
            let conflict = match dialect {
                Dialect::Postgres | Dialect::Sqlite => {
                    let mut updates = upsert
                        .sets
                        .iter()
                        .map(|set| format!("{0} = EXCLUDED.{0}", set.field))
                        .collect::<Vec<_>>();
                    if SOURCE_AUTO_UPDATED.with(|sources| sources.borrow().contains(&upsert.source))
                        && !upsert.sets.iter().any(|set| set.field == "updated_at")
                    {
                        updates.push(format!("updated_at = {}", dialect.now_expr()));
                    }
                    format!(
                        " ON CONFLICT ({}) DO UPDATE SET {}",
                        upsert
                            .keys
                            .iter()
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        updates.join(", ")
                    )
                }
                Dialect::Mysql => {
                    let mut updates = upsert
                        .sets
                        .iter()
                        .map(|set| format!("{0} = VALUES({0})", set.field))
                        .collect::<Vec<_>>();
                    if SOURCE_AUTO_UPDATED.with(|sources| sources.borrow().contains(&upsert.source))
                        && !upsert.sets.iter().any(|set| set.field == "updated_at")
                    {
                        updates.push(format!("updated_at = {}", dialect.now_expr()));
                    }
                    format!(" ON DUPLICATE KEY UPDATE {}", updates.join(", "))
                }
            };
            let returning = if upsert.binding.is_some() {
                dialect.returning_star()
            } else {
                ""
            };
            writeln!(out, "{pad}let _upsert = db_try!(sqlx::query(\"INSERT INTO {src} ({cols}) VALUES ({values}){conflict}{returning}\")",
                src = upsert.source, cols = cols.join(", "), values = placeholders.join(", ")).unwrap();
            for (column, expr) in &fields {
                let bind = lower_expr_sql_bind_typed(expr, &upsert.source, column);
                writeln!(out, "{pad}    .bind({bind})").unwrap();
            }
            if let Some(binding) = &upsert.binding {
                if dialect == Dialect::Mysql {
                    writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
                    let where_clause = upsert
                        .keys
                        .iter()
                        .map(|(name, _)| format!("{name} = ?"))
                        .collect::<Vec<_>>()
                        .join(" AND ");
                    writeln!(out, "{pad}let _upsert_row = db_try!(sqlx::query(\"SELECT * FROM {src} WHERE {where_clause} LIMIT 1\")", src = upsert.source).unwrap();
                    for (column, expr) in &upsert.keys {
                        let bind = lower_expr_sql_bind_typed(expr, &upsert.source, column);
                        writeln!(out, "{pad}    .bind({bind})").unwrap();
                    }
                    writeln!(out, "{pad}    .fetch_one({pool_ref}).await);").unwrap();
                    writeln!(out, "{pad}let {binding} = row_to_json(&_upsert_row);").unwrap();
                } else {
                    writeln!(out, "{pad}    .fetch_one({pool_ref}).await);").unwrap();
                    writeln!(out, "{pad}let {binding} = row_to_json(&_upsert);").unwrap();
                }
            } else {
                writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
            }
            writeln!(
                out,
                "{pad}tracing::info!(source = \"{src}\", op = \"UPSERT\", \"mutation executed\");",
                src = upsert.source
            )
            .unwrap();
        }
        FlowStep::Update(upd) => {
            let dialect = source_dialect(&upd.source);
            let pool_ref = db_ref_for(&upd.source);
            let has_updated_at =
                SOURCE_AUTO_UPDATED.with(|sources| sources.borrow().contains(&upd.source));
            let explicit_sets_count = upd.sets.len();
            let mut sets: Vec<String> = upd
                .sets
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let ph = dialect.ph(i + 1);
                    if is_body_ref(&s.value) {
                        format!("{f} = COALESCE({ph}, {f})", f = s.field)
                    } else {
                        format!("{} = {ph}", s.field)
                    }
                })
                .collect();
            if has_updated_at && !upd.sets.iter().any(|s| s.field == "updated_at") {
                sets.push(format!("updated_at = {}", dialect.now_expr()));
            }
            let wheres: Vec<String> = upd
                .wheres
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let op = sql_compare_op(&w.op);
                    let ph = dialect.ph(i + 1 + explicit_sets_count);
                    format!("{} {op} {ph}", w.field)
                })
                .collect();
            let where_clause = if wheres.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", wheres.join(" AND "))
            };
            let returning = match &upd.binding {
                Some(UpdateBinding::As(_)) => dialect.returning_star(),
                _ => "",
            };
            writeln!(out, "{pad}let _upd = db_try!(sqlx::query(\"UPDATE {src} SET {sets}{where_clause}{returning}\")",
                src = upd.source, sets = sets.join(", ")).unwrap();
            for s in &upd.sets {
                if is_body_ref(&s.value) {
                    if let Expr::DotPath(dp) = &s.value {
                        let path = dp.segments.join(".");
                        writeln!(out, "{pad}    .bind({path})").unwrap();
                    }
                } else {
                    let bind = lower_expr_sql_bind_typed(&s.value, &upd.source, &s.field);
                    writeln!(out, "{pad}    .bind({bind})").unwrap();
                }
            }
            for w in &upd.wheres {
                let bind = lower_expr_sql_bind_typed(&w.value, &upd.source, &w.field);
                writeln!(out, "{pad}    .bind({bind})").unwrap();
            }
            match &upd.binding {
                Some(UpdateBinding::As(name)) => {
                    if dialect == Dialect::Mysql {
                        writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
                        writeln!(out, "{pad}if _upd.rows_affected() == 0 {{").unwrap();
                        let msg = upd.or_message.as_deref().unwrap_or("not found");
                        let sc = status_code_expr(upd.or_code);
                        writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                        writeln!(out, "{pad}}}").unwrap();
                        writeln!(
                            out,
                            "{pad}let {name} = serde_json::json!({{\"updated\": true}});"
                        )
                        .unwrap();
                    } else {
                        writeln!(out, "{pad}    .fetch_optional({pool_ref}).await);").unwrap();
                        writeln!(out, "{pad}let {name} = match _upd {{").unwrap();
                        writeln!(out, "{pad}    Some(row) => row_to_json(&row),").unwrap();
                        let msg = upd.or_message.as_deref().unwrap_or("not found");
                        let sc = status_code_expr(upd.or_code);
                        writeln!(out, "{pad}    None => return api_error({sc}, \"{msg}\"),")
                            .unwrap();
                        writeln!(out, "{pad}}};").unwrap();
                    }
                }
                Some(UpdateBinding::Count(name)) => {
                    writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
                    writeln!(
                        out,
                        "{pad}let {name} = serde_json::json!(_upd.rows_affected());"
                    )
                    .unwrap();
                }
                None => {
                    writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
                    if upd.or_code > 0 {
                        let msg = upd.or_message.as_deref().unwrap_or("not found");
                        let sc = status_code_expr(upd.or_code);
                        writeln!(out, "{pad}if _upd.rows_affected() == 0 {{").unwrap();
                        writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                        writeln!(out, "{pad}}}").unwrap();
                    }
                }
            }
            writeln!(
                out,
                "{pad}tracing::info!(source = \"{src}\", op = \"UPDATE\", \"mutation executed\");",
                src = upd.source
            )
            .unwrap();
        }
        FlowStep::Delete(del) => {
            let dialect = source_dialect(&del.source);
            let pool_ref = db_ref_for(&del.source);
            let wheres: Vec<String> = del
                .wheres
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let op = sql_compare_op(&w.op);
                    let ph = dialect.ph(i + 1);
                    format!("{} {op} {ph}", w.field)
                })
                .collect();
            let where_clause = if wheres.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", wheres.join(" AND "))
            };
            writeln!(
                out,
                "{pad}let _del = db_try!(sqlx::query(\"DELETE FROM {src}{where_clause}\")",
                src = del.source
            )
            .unwrap();
            for w in &del.wheres {
                let bind = lower_expr_sql_bind_typed(&w.value, &del.source, &w.field);
                writeln!(out, "{pad}    .bind({bind})").unwrap();
            }
            writeln!(out, "{pad}    .execute({pool_ref}).await);").unwrap();
            if del.or_code > 0 {
                let msg = del.or_message.as_deref().unwrap_or("not found");
                let sc = status_code_expr(del.or_code);
                writeln!(out, "{pad}if _del.rows_affected() == 0 {{").unwrap();
                writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
            writeln!(
                out,
                "{pad}tracing::info!(source = \"{src}\", op = \"DELETE\", \"mutation executed\");",
                src = del.source
            )
            .unwrap();
        }
        FlowStep::Guard(g) => {
            let msg = g.message.as_deref().unwrap_or("guard failed");
            let sc = status_code_expr(g.code);
            if let Expr::Unary {
                op: UnaryOp::Empty,
                operand,
            } = &g.expr
            {
                if let Expr::Query {
                    source,
                    filters,
                    sorts,
                    cursor,
                    page_size,
                    ..
                } = operand.as_ref()
                {
                    let dialect = source_dialect(source);
                    let pool_ref = db_ref_for(source);
                    let where_clause = sql_filters_for(filters, dialect);
                    let order_clause = if sorts.is_empty() {
                        String::new()
                    } else {
                        let parts: Vec<String> = sorts
                            .iter()
                            .map(|s| {
                                let dir = match s.direction {
                                    SortDirection::Asc => "ASC",
                                    SortDirection::Desc => "DESC",
                                };
                                format!("{} {dir}", s.field)
                            })
                            .collect();
                        format!(" ORDER BY {}", parts.join(", "))
                    };
                    let mut param_idx = filters.len() + 1;
                    let mut sql = format!(
                        "SELECT COUNT(*) as count FROM {source}{where_clause}{order_clause}"
                    );
                    if cursor.is_some() {
                        let ph = dialect.ph(param_idx);
                        if where_clause.is_empty() {
                            sql.push_str(&format!(" WHERE id > {ph}"));
                        } else {
                            sql.push_str(&format!(" AND id > {ph}"));
                        }
                        param_idx += 1;
                    }
                    if page_size.is_some() {
                        let ph = dialect.ph(param_idx);
                        sql.push_str(&format!(" LIMIT {ph}"));
                        param_idx += 1;
                    }
                    let _ = param_idx;
                    writeln!(
                        out,
                        "{pad}let _guard_count = db_try!(sqlx::query_scalar::<_, i64>(\"{sql}\")"
                    )
                    .unwrap();
                    for f in filters {
                        emit_filter_bind(out, f, source, &pad);
                    }
                    if let Some(c) = cursor {
                        let bind = lower_expr_sql_bind(c);
                        writeln!(out, "{pad}    .bind({bind})").unwrap();
                    }
                    if let Some(ps) = page_size {
                        let bind = lower_expr_sql_bind(ps);
                        writeln!(out, "{pad}    .bind({bind})").unwrap();
                    }
                    writeln!(out, "{pad}    .fetch_one({pool_ref}).await);").unwrap();
                    writeln!(out, "{pad}if _guard_count != 0 {{").unwrap();
                    writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                    writeln!(out, "{pad}}}").unwrap();
                } else {
                    let cond = lower_expr_bool(&g.expr);
                    writeln!(out, "{pad}if !({cond}) {{").unwrap();
                    writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                    writeln!(out, "{pad}}}").unwrap();
                }
            } else {
                let cond = lower_expr_bool(&g.expr);
                writeln!(out, "{pad}if !({cond}) {{").unwrap();
                writeln!(out, "{pad}    return api_error({sc}, \"{msg}\");").unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
        }
        FlowStep::Rule(r) => {
            for check in &r.requires {
                let cond = lower_rule_check(&check.path, &check.op, &check.value);
                writeln!(out, "{pad}if !({cond}) {{").unwrap();
                writeln!(out, "{pad}    return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{name}: {field} validation failed\");",
                    name = r.name, field = check.path.as_str()).unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
        }
        FlowStep::Effect(e) => {
            let first_src = all_sql_sources()
                .into_iter()
                .next()
                .map(|(n, _)| n)
                .unwrap_or_else(|| "default".into());
            let outbox_dialect = DB_DIALECT_OVERRIDE
                .with(|value| value.borrow().unwrap_or_else(|| source_dialect(&first_src)));
            let outbox_pool = db_ref_for(&first_src);
            let placeholders = (1..=9)
                .map(|index| outbox_dialect.ph(index))
                .collect::<Vec<_>>()
                .join(", ");
            let kind = match e.kind {
                EffectKind::Email => "email",
                EffectKind::PushNotification => "push",
                EffectKind::Async => "async_task",
                EffectKind::Webhook => "webhook",
            };
            let outbox_id = if outbox_dialect == Dialect::Postgres {
                "uuid::Uuid::new_v4()"
            } else {
                "uuid::Uuid::new_v4().to_string()"
            };
            let mut payload_fields = vec![format!("\"kind\": \"{kind}\"")];
            for field in &e.fields {
                match field {
                    EffectField::Template(value) => {
                        payload_fields.push(format!("\"template\": {:?}", value))
                    }
                    EffectField::To(value) => {
                        payload_fields.push(format!("\"to\": {}", lower_expr(value)))
                    }
                    EffectField::Data(values) => payload_fields.push(format!(
                        "\"data\": [{}]",
                        values.iter().map(lower_expr).collect::<Vec<_>>().join(", ")
                    )),
                    EffectField::Url(value) => {
                        payload_fields.push(format!("\"url\": {}", lower_expr(value)))
                    }
                    EffectField::Event(value) => {
                        payload_fields.push(format!("\"event\": {:?}", value))
                    }
                    EffectField::Task(value) => {
                        payload_fields.push(format!("\"task\": {:?}", value))
                    }
                }
            }
            writeln!(
                out,
                "{pad}let _axis_effect = serde_json::json!({{{}}});",
                payload_fields.join(", ")
            )
            .unwrap();
            writeln!(out, "{pad}let _axis_effect_string = |name: &str| _axis_effect.get(name).map(|value| value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()));").unwrap();
            writeln!(
                out,
                "{pad}let _axis_effect_payload = _axis_effect.to_string();"
            )
            .unwrap();
            writeln!(out, "{pad}db_try!(sqlx::query(\"INSERT INTO _axis_outbox (id, kind, template, recipient, url, event, task, payload, status) VALUES ({placeholders})\")").unwrap();
            writeln!(out, "{pad}    .bind({outbox_id}).bind(\"{kind}\")").unwrap();
            writeln!(out, "{pad}    .bind(_axis_effect_string(\"template\")).bind(_axis_effect_string(\"to\")).bind(_axis_effect_string(\"url\"))").unwrap();
            writeln!(out, "{pad}    .bind(_axis_effect_string(\"event\")).bind(_axis_effect_string(\"task\")).bind(_axis_effect_payload).bind(\"pending\")").unwrap();
            writeln!(out, "{pad}    .execute({outbox_pool}).await);").unwrap();
        }
        FlowStep::Match(m) => {
            for (i, branch) in m.branches.iter().enumerate() {
                let cond = lower_expr_bool(&branch.condition);
                if i == 0 {
                    writeln!(out, "{pad}if {cond} {{").unwrap();
                } else {
                    writeln!(out, "{pad}}} else if {cond} {{").unwrap();
                }
                generate_flow_steps(out, &branch.steps, indent + 4, db_ref);
            }
            if let Some(default) = &m.default {
                writeln!(out, "{pad}}} else {{").unwrap();
                generate_flow_steps(out, default, indent + 4, db_ref);
            }
            writeln!(out, "{pad}}}").unwrap();
        }
        FlowStep::Each(each) => {
            let src = lower_expr(&each.source);
            if let Some(n) = each.parallel {
                writeln!(out, "{pad}// PARALLEL({n})").unwrap();
            }
            writeln!(out, "{pad}if let Some(_items) = ({src}).as_array() {{").unwrap();
            writeln!(
                out,
                "{pad}    for _{binding} in _items {{",
                binding = each.binding
            )
            .unwrap();
            writeln!(
                out,
                "{pad}        let {binding} = _{binding}.clone();",
                binding = each.binding
            )
            .unwrap();
            generate_flow_steps(out, &each.steps, indent + 8, db_ref);
            writeln!(out, "{pad}    }}").unwrap();
            writeln!(out, "{pad}}}").unwrap();
        }
        FlowStep::Fanout(fanout) => {
            let dialect = source_dialect(&fanout.insert.source);
            let pool_ref = db_ref_for(&fanout.insert.source);
            let backend = match dialect {
                Dialect::Postgres => "sqlx::Postgres",
                Dialect::Mysql => "sqlx::MySql",
                Dialect::Sqlite => "sqlx::Sqlite",
            };
            let source = lower_expr(&fanout.source);
            let max_parameters = match dialect {
                Dialect::Postgres | Dialect::Mysql => 65_535usize,
                Dialect::Sqlite => 32_766usize,
            };
            let row_width = fanout.insert.fields.len();
            let columns = fanout
                .insert
                .fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{pad}let _fanout_value = {source};").unwrap();
            writeln!(out, "{pad}let _fanout_items = _fanout_value.as_array().or_else(|| _fanout_value.get(\"items\").and_then(serde_json::Value::as_array)).or_else(|| _fanout_value.get(\"data\").and_then(serde_json::Value::as_array)).cloned().unwrap_or_default();").unwrap();
            writeln!(out, "{pad}if _fanout_items.len().saturating_mul({row_width}) > {max_parameters} {{ return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"FANOUT exceeds the database parameter limit\"); }}").unwrap();
            writeln!(out, "{pad}if !_fanout_items.is_empty() {{").unwrap();
            writeln!(out, "{pad}    let mut _fanout = sqlx::QueryBuilder::<{backend}>::new(\"INSERT INTO {src} ({columns}) \" );", src = fanout.insert.source).unwrap();
            writeln!(
                out,
                "{pad}    _fanout.push_values(_fanout_items.clone(), |mut row, {binding}| {{",
                binding = fanout.binding
            )
            .unwrap();
            FLOW_SCOPE.with(|scope| {
                scope.borrow_mut().insert(fanout.binding.clone());
            });
            for (column, expr) in &fanout.insert.fields {
                let bind = lower_expr_sql_bind_typed(expr, &fanout.insert.source, column);
                writeln!(out, "{pad}        row.push_bind({bind});").unwrap();
            }
            FLOW_SCOPE.with(|scope| {
                scope.borrow_mut().remove(&fanout.binding);
            });
            writeln!(out, "{pad}    }});").unwrap();
            writeln!(
                out,
                "{pad}    db_try!(_fanout.build().execute({pool_ref}).await);"
            )
            .unwrap();
            writeln!(out, "{pad}}}").unwrap();
            writeln!(out, "{pad}tracing::info!(source = \"{src}\", op = \"FANOUT\", count = _fanout_items.len(), \"mutation executed\");", src = fanout.insert.source).unwrap();
        }
        FlowStep::Try(t) => {
            let mut try_body = String::new();
            generate_flow_steps(&mut try_body, &t.body, indent + 4, db_ref);
            let try_body = try_body.replace(
                "return api_error(",
                "break 'try_block Some(\"recoverable error\".to_string());\n            #[allow(unreachable_code)] return api_error("
            );
            writeln!(out, "{pad}let _try_err: Option<String> = 'try_block: {{").unwrap();
            out.push_str(&try_body);
            writeln!(out, "{pad}    None").unwrap();
            writeln!(out, "{pad}}};").unwrap();
            if !t.recover.is_empty() {
                writeln!(out, "{pad}if _try_err.is_some() {{").unwrap();
                generate_flow_steps(out, &t.recover, indent + 4, db_ref);
                writeln!(out, "{pad}}}").unwrap();
            }
        }
        FlowStep::Upload(u) => {
            let storage = STORAGES.with(|storages| storages.borrow().get(&u.storage).cloned());
            let Some(storage) = storage else {
                writeln!(out, "{pad}return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"undefined upload storage\");").unwrap();
                return;
            };
            let value = lower_expr(&u.file_expr);
            let allowed_types = storage
                .types
                .iter()
                .map(|value| serde_json::to_string(value).expect("storage type is serializable"))
                .collect::<Vec<_>>()
                .join(", ");
            let bucket =
                serde_json::to_string(&storage.bucket).expect("storage bucket is serializable");
            let prefix = storage
                .prefix
                .as_deref()
                .map(|value| serde_json::to_string(value).expect("storage prefix is serializable"));
            writeln!(out, "{pad}let _axis_upload_value = {value};").unwrap();
            writeln!(out, "{pad}let (_axis_upload_bytes, _axis_upload_filename, _axis_upload_content_type) = match axis_upload_parts(&_axis_upload_value) {{ Ok(parts) => parts, Err(message) => return api_error(StatusCode::BAD_REQUEST, &message) }};").unwrap();
            if let Some(max_size) = storage.max_size {
                writeln!(out, "{pad}if _axis_upload_bytes.len() > {max_size}_usize {{ return api_error(StatusCode::PAYLOAD_TOO_LARGE, \"file exceeds the storage size limit\"); }}").unwrap();
            }
            writeln!(out, "{pad}let _axis_upload_extension = match axis_safe_upload_extension(&_axis_upload_bytes, _axis_upload_filename.as_deref(), _axis_upload_content_type.as_deref(), &[{allowed_types}]) {{ Ok(extension) => extension, Err(message) => return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, &message) }};").unwrap();
            writeln!(out, "{pad}let _axis_upload_filename = format!(\"{{}}.{{}}\", uuid::Uuid::new_v4(), _axis_upload_extension);").unwrap();
            if let Some(prefix) = prefix {
                writeln!(out, "{pad}let _axis_upload_key = format!(\"{{}}/{{}}\", {prefix}.trim_matches('/'), _axis_upload_filename);").unwrap();
            } else {
                writeln!(
                    out,
                    "{pad}let _axis_upload_key = _axis_upload_filename.clone();"
                )
                .unwrap();
            }
            match storage.backend {
                StorageBackend::Local => {
                    if let Some(prefix) = storage.prefix.as_deref() {
                        let prefix =
                            serde_json::to_string(prefix).expect("storage prefix is serializable");
                        writeln!(out, "{pad}let _axis_upload_dir = std::path::Path::new({bucket}).join({prefix});").unwrap();
                    } else {
                        writeln!(
                            out,
                            "{pad}let _axis_upload_dir = std::path::PathBuf::from({bucket});"
                        )
                        .unwrap();
                    }
                    writeln!(out, "{pad}if let Err(error) = tokio::fs::create_dir_all(&_axis_upload_dir).await {{ tracing::error!(%error, \"failed to create upload directory\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"storage write failed\"); }}").unwrap();
                    writeln!(out, "{pad}if let Err(error) = tokio::fs::write(_axis_upload_dir.join(&_axis_upload_filename), &_axis_upload_bytes).await {{ tracing::error!(%error, \"failed to write upload\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"storage write failed\"); }}").unwrap();
                    if storage.access == StorageAccess::Public {
                        writeln!(out, "{pad}let {} = serde_json::json!(format!(\"/files/{}/{{}}\", _axis_upload_key));", u.binding, storage.name).unwrap();
                    } else {
                        writeln!(
                            out,
                            "{pad}let {} = serde_json::json!(_axis_upload_key);",
                            u.binding
                        )
                        .unwrap();
                    }
                }
                StorageBackend::S3 => {
                    writeln!(out, "{pad}let _axis_upload_path = match object_store::path::Path::parse(&_axis_upload_key) {{ Ok(path) => path, Err(error) => {{ tracing::error!(%error, \"invalid S3 object path\"); return api_error(StatusCode::INTERNAL_SERVER_ERROR, \"storage write failed\"); }} }};").unwrap();
                    writeln!(out, "{pad}if let Err(error) = object_store::ObjectStore::put_opts(&*state.storage_{}, &_axis_upload_path, _axis_upload_bytes.into(), object_store::PutOptions::default()).await {{ tracing::error!(%error, \"S3 upload failed\"); return api_error(StatusCode::BAD_GATEWAY, \"storage write failed\"); }}", storage.name).unwrap();
                    if storage.access == StorageAccess::Public {
                        writeln!(out, "{pad}let {} = serde_json::json!(axis_s3_public_url(\"{}\", {bucket}, &_axis_upload_key));", u.binding, storage.name).unwrap();
                    } else {
                        writeln!(out, "{pad}let {} = serde_json::json!(format!(\"s3://{{}}/{{}}\", {bucket}, _axis_upload_key));", u.binding).unwrap();
                    }
                }
            }
            writeln!(out, "{pad}tracing::info!(storage = \"{}\", object = %_axis_upload_key, \"file uploaded\");", storage.name).unwrap();
        }
    }
}

fn generate_let_step(out: &mut String, l: &LetStep, indent: usize, _db_ref: &str) {
    let pad = " ".repeat(indent);
    let name = &l.name;
    match &l.expr {
        Expr::Fetch {
            source,
            filters,
            with,
            or_code,
            or_message,
            or_shape,
        } => {
            let dialect = source_dialect(source);
            let pool_ref = db_ref_for(source);
            let where_clause = sql_filters_for(filters, dialect);
            let join_clause = if with.is_empty() {
                String::new()
            } else {
                let joins: Vec<String> = with
                    .iter()
                    .map(|w| format!(" LEFT JOIN {w} ON {source}.id = {w}.{source}_id"))
                    .collect();
                joins.join("")
            };
            let cols = if join_clause.is_empty() {
                source_columns(source)
            } else {
                "*".to_string()
            };
            writeln!(out, "{pad}let {name} = db_try!(sqlx::query(\"SELECT {cols} FROM {source}{join_clause}{where_clause} LIMIT 1\")").unwrap();
            for f in filters {
                emit_filter_bind(out, f, source, &pad);
            }
            writeln!(out, "{pad}    .fetch_optional({pool_ref}).await);").unwrap();
            let msg = or_message.as_deref().unwrap_or("not found");
            writeln!(out, "{pad}let {name} = match {name} {{").unwrap();
            writeln!(out, "{pad}    Some(row) => row_to_json(&row),").unwrap();
            let sc = status_code_expr(*or_code);
            if let Some(err_shape) = or_shape {
                let err_fields: Vec<String> = err_shape
                    .fields
                    .iter()
                    .map(|(k, v)| format!("\"{k}\": {}", lower_expr(v)))
                    .collect();
                writeln!(out, "{pad}    None => return ({sc}, Json(serde_json::json!({{\"error\": \"{msg}\", \"code\": {or_code}, \"shape\": \"{shape}\", \"request_id\": uuid::Uuid::new_v4().to_string(), {fields}}}))).into_response(),",
                    shape = err_shape.shape, fields = err_fields.join(", ")).unwrap();
            } else {
                writeln!(out, "{pad}    None => return api_error({sc}, \"{msg}\"),").unwrap();
            }
            writeln!(out, "{pad}}};").unwrap();
        }
        Expr::Query {
            source,
            filters,
            sorts,
            cursor,
            page_size,
            ..
        } => {
            let dialect = source_dialect(source);
            let pool_ref = db_ref_for(source);
            let where_clause = sql_filters_for(filters, dialect);
            let order_clause = if sorts.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = sorts
                    .iter()
                    .map(|s| {
                        let dir = match s.direction {
                            SortDirection::Asc => "ASC",
                            SortDirection::Desc => "DESC",
                        };
                        format!("{} {dir}", s.field)
                    })
                    .collect();
                format!(" ORDER BY {}", parts.join(", "))
            };
            let mut param_idx = filters.len() + 1;
            let cols = source_columns(source);
            let mut sql = format!("SELECT {cols} FROM {source}{where_clause}{order_clause}");
            if cursor.is_some() {
                let ph = dialect.ph(param_idx);
                if where_clause.is_empty() {
                    sql.push_str(&format!(" WHERE id > {ph}"));
                } else {
                    sql.push_str(&format!(" AND id > {ph}"));
                }
                param_idx += 1;
            }
            if page_size.is_some() {
                let ph1 = dialect.ph(param_idx);
                sql.push_str(&format!(" LIMIT {ph1}"));
                param_idx += 1;
                let ph2 = dialect.ph(param_idx);
                sql.push_str(&format!(" OFFSET {ph2}"));
                param_idx += 1;
            }
            let _ = param_idx;
            writeln!(out, "{pad}let {name} = db_try!(sqlx::query(\"{sql}\")").unwrap();
            for f in filters {
                emit_filter_bind(out, f, source, &pad);
            }
            if let Some(c) = cursor {
                let bind = lower_expr_sql_bind(c);
                writeln!(out, "{pad}    .bind({bind})").unwrap();
            }
            if let Some(ps) = page_size {
                let mut bind = lower_expr_sql_bind(ps);
                if bind.contains("unwrap_or_default()") {
                    bind = bind.replace("unwrap_or_default()", "unwrap_or(100)");
                }
                let bind = format!("({bind}).min(1000)");
                writeln!(out, "{pad}    .bind({bind})").unwrap();
                writeln!(
                    out,
                    "{pad}    .bind((query.page.unwrap_or(1).max(1) - 1).saturating_mul({bind}))"
                )
                .unwrap();
            }
            writeln!(out, "{pad}    .fetch_all({pool_ref}).await);").unwrap();
            writeln!(out, "{pad}let {name}: Vec<serde_json::Value> = {name}.iter().map(|r| row_to_json(r)).collect();").unwrap();
            if page_size.is_some() {
                let count_sql = format!("SELECT COUNT(*) as count FROM {source}{where_clause}");
                writeln!(
                    out,
                    "{pad}let {name}_total = db_try!(sqlx::query_scalar::<_, i64>(\"{count_sql}\")"
                )
                .unwrap();
                for f in filters {
                    emit_filter_bind(out, f, source, &pad);
                }
                writeln!(out, "{pad}    .fetch_one({pool_ref}).await);").unwrap();
                let ps_bind = lower_expr_sql_bind(page_size.as_ref().unwrap());
                let ps_val = if ps_bind.contains("unwrap_or_default()") {
                    ps_bind.replace("unwrap_or_default()", "unwrap_or(100)")
                } else {
                    ps_bind
                };
                writeln!(
                    out,
                    "{pad}let {name}_page_size = ({ps_val} as i64).min(1000);"
                )
                .unwrap();
                writeln!(
                    out,
                    "{pad}let {name}_page = query.page.unwrap_or(1).max(1);"
                )
                .unwrap();
                writeln!(out, "{pad}let {name}_len = {name}.len() as i64;").unwrap();
                writeln!(out, "{pad}let {name} = serde_json::json!({{\"data\": {name}, \"total\": {name}_total, \"page\": {name}_page, \"page_size\": {name}_page_size, \"has_more\": ({name}_len >= {name}_page_size)}});").unwrap();
            } else {
                writeln!(out, "{pad}let {name} = serde_json::json!({name});").unwrap();
            }
        }
        Expr::Call {
            service,
            method,
            args,
            ..
        } => {
            let arg_json: Vec<String> = args
                .iter()
                .map(|(k, v)| format!("\"{k}\": {}", lower_expr(v)))
                .collect();
            writeln!(out, "{pad}let {name} = serde_json::json!({{\"_service_call\": \"{service}.{method}\", {}}});", arg_json.join(", ")).unwrap();
        }
        _ => {
            let val = lower_expr(&l.expr);
            writeln!(out, "{pad}let {name} = {val};").unwrap();
        }
    }
}

fn status_code_expr(code: i64) -> String {
    match code {
        200 => "StatusCode::OK".into(),
        201 => "StatusCode::CREATED".into(),
        202 => "StatusCode::ACCEPTED".into(),
        204 => "StatusCode::NO_CONTENT".into(),
        301 => "StatusCode::MOVED_PERMANENTLY".into(),
        302 => "StatusCode::FOUND".into(),
        304 => "StatusCode::NOT_MODIFIED".into(),
        400 => "StatusCode::BAD_REQUEST".into(),
        401 => "StatusCode::UNAUTHORIZED".into(),
        403 => "StatusCode::FORBIDDEN".into(),
        404 => "StatusCode::NOT_FOUND".into(),
        409 => "StatusCode::CONFLICT".into(),
        422 => "StatusCode::UNPROCESSABLE_ENTITY".into(),
        429 => "StatusCode::TOO_MANY_REQUESTS".into(),
        500 => "StatusCode::INTERNAL_SERVER_ERROR".into(),
        502 => "StatusCode::BAD_GATEWAY".into(),
        503 => "StatusCode::SERVICE_UNAVAILABLE".into(),
        _ => format!("StatusCode::from_u16({code} as u16).unwrap()"),
    }
}

fn generate_return_stmt(out: &mut String, ret: &ReturnStmt, indent: usize) {
    if ret.headers.is_empty() {
        generate_return_body(out, ret, indent);
        return;
    }
    let pad = " ".repeat(indent);
    writeln!(out, "{pad}let mut _return_response = {{").unwrap();
    generate_return_body(out, ret, indent + 4);
    writeln!(out, "{pad}}};").unwrap();
    for (name, expression) in &ret.headers {
        let value = lower_expr(expression);
        writeln!(out, "{pad}let _return_header_value = {value};").unwrap();
        writeln!(out, "{pad}let _return_header_value = _return_header_value.as_str().map(str::to_owned).unwrap_or_else(|| _return_header_value.to_string());").unwrap();
        writeln!(
            out,
            "{pad}if let Ok(value) = _return_header_value.parse::<axum::http::HeaderValue>() {{"
        )
        .unwrap();
        writeln!(
            out,
            "{pad}    _return_response.headers_mut().insert(\"{}\", value);",
            name.to_ascii_lowercase()
        )
        .unwrap();
        writeln!(out, "{pad}}}").unwrap();
    }
    writeln!(out, "{pad}_return_response").unwrap();
}

fn generate_return_body(out: &mut String, ret: &ReturnStmt, indent: usize) {
    let pad = " ".repeat(indent);
    let sc = status_code_expr(ret.code);
    match &ret.body {
        Some(ReturnBody::Binding(name)) => {
            writeln!(out, "{pad}({sc}, Json({name})).into_response()").unwrap();
        }
        Some(ReturnBody::Inline(fields)) => {
            writeln!(out, "{pad}({sc}, Json(serde_json::json!({{").unwrap();
            for f in fields {
                let val = match &f.value {
                    ReturnValue::Expr(e) => lower_expr(e),
                    ReturnValue::Nested(nested) => {
                        let entries: Vec<String> = nested
                            .iter()
                            .map(|n| {
                                let v = match &n.value {
                                    ReturnValue::Expr(e) => lower_expr(e),
                                    ReturnValue::Nested(_) => "serde_json::Value::Null".into(),
                                };
                                format!("\"{}\": {v}", n.name)
                            })
                            .collect();
                        format!("serde_json::json!({{{}}})", entries.join(", "))
                    }
                };
                writeln!(out, "{pad}    \"{name}\": {val},", name = f.name).unwrap();
            }
            writeln!(out, "{pad}}}))).into_response()").unwrap();
        }
        Some(ReturnBody::Paginated {
            items,
            total,
            cursor,
            has_more,
        }) => {
            let items_expr = lower_expr(items);
            let total_expr = lower_expr(total);
            let cursor_expr = lower_expr(cursor);
            let has_more_expr = lower_expr(has_more);
            writeln!(out, "{pad}({sc}, Json(serde_json::json!({{").unwrap();
            writeln!(out, "{pad}    \"items\": {items_expr},").unwrap();
            writeln!(out, "{pad}    \"total\": {total_expr},").unwrap();
            writeln!(out, "{pad}    \"cursor\": {cursor_expr},").unwrap();
            writeln!(out, "{pad}    \"has_more\": {has_more_expr},").unwrap();
            writeln!(out, "{pad}}}))).into_response()").unwrap();
        }
        None => {
            writeln!(out, "{pad}{sc}.into_response()").unwrap();
        }
    }
}

fn generate_body_validation(out: &mut String, body: &BodyDecl, indent: usize) {
    let pad = " ".repeat(indent);
    for field in &body.fields {
        let is_required = field
            .modifiers
            .iter()
            .any(|m| matches!(m, Modifier::Required));
        let is_string = matches!(&field.ty, TypeExpr::String(_) | TypeExpr::Text);
        if is_string {
            if is_required {
                writeln!(out, "{pad}body.{name} = body.{name}.trim().chars().filter(|c| !c.is_control() || *c == '\\n').collect();",
                    name = field.name).unwrap();
            } else {
                writeln!(out, "{pad}body.{name} = body.{name}.take().map(|s| s.trim().chars().filter(|c| !c.is_control() || *c == '\\n').collect());",
                    name = field.name).unwrap();
            }
        }
        for m in &field.modifiers {
            match m {
                Modifier::Min(min) => {
                    if is_required {
                        if is_string {
                            writeln!(
                                out,
                                "{pad}if body.{name}.len() < {min} {{",
                                name = field.name
                            )
                            .unwrap();
                        } else {
                            writeln!(out, "{pad}if body.{name} < {min} {{", name = field.name)
                                .unwrap();
                        }
                    } else if is_string {
                        writeln!(
                            out,
                            "{pad}if body.{name}.as_ref().map_or(false, |v| v.len() < {min}) {{",
                            name = field.name
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            out,
                            "{pad}if body.{name}.map_or(false, |v| v < {min}) {{",
                            name = field.name
                        )
                        .unwrap();
                    }
                    let desc = if is_string { "length" } else { "value" };
                    writeln!(out, "{pad}    return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{name}: minimum {desc} is {min}\");",
                        name = field.name).unwrap();
                    writeln!(out, "{pad}}}").unwrap();
                }
                Modifier::Max(max) => {
                    if is_required {
                        if is_string {
                            writeln!(
                                out,
                                "{pad}if body.{name}.len() > {max} {{",
                                name = field.name
                            )
                            .unwrap();
                        } else {
                            writeln!(out, "{pad}if body.{name} > {max} {{", name = field.name)
                                .unwrap();
                        }
                    } else if is_string {
                        writeln!(
                            out,
                            "{pad}if body.{name}.as_ref().map_or(false, |v| v.len() > {max}) {{",
                            name = field.name
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            out,
                            "{pad}if body.{name}.map_or(false, |v| v > {max}) {{",
                            name = field.name
                        )
                        .unwrap();
                    }
                    let desc = if is_string { "length" } else { "value" };
                    writeln!(out, "{pad}    return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{name}: maximum {desc} is {max}\");",
                        name = field.name).unwrap();
                    writeln!(out, "{pad}}}").unwrap();
                }
                _ => {}
            }
        }
        if let TypeExpr::Int { min, max } = &field.ty {
            if let Some(min) = min {
                if is_required {
                    writeln!(out, "{pad}if body.{name} < {min} {{", name = field.name).unwrap();
                } else {
                    writeln!(
                        out,
                        "{pad}if body.{name}.map_or(false, |v| v < {min}) {{",
                        name = field.name
                    )
                    .unwrap();
                }
                writeln!(out, "{pad}    return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{name}: minimum value is {min}\");",
                    name = field.name).unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
            if let Some(max) = max {
                if is_required {
                    writeln!(out, "{pad}if body.{name} > {max} {{", name = field.name).unwrap();
                } else {
                    writeln!(
                        out,
                        "{pad}if body.{name}.map_or(false, |v| v > {max}) {{",
                        name = field.name
                    )
                    .unwrap();
                }
                writeln!(out, "{pad}    return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{name}: maximum value is {max}\");",
                    name = field.name).unwrap();
                writeln!(out, "{pad}}}").unwrap();
            }
        }
        if let TypeExpr::Decimal {
            precision: _,
            scale: _,
        } = &field.ty
        {
            // Decimal constraints handled by Modifier::Min/Max
        }
    }
}

fn generate_multipart_body_parse(out: &mut String, flow: &FlowDef, body: &BodyDecl) {
    let body_name = format!("{}Body", pascal(&flow.name));
    writeln!(
        out,
        "    let mut _axis_multipart = match Multipart::from_request(request, &state).await {{"
    )
    .unwrap();
    writeln!(out, "        Ok(multipart) => multipart,").unwrap();
    writeln!(out, "        Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart body: {{error}}\")),").unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    let mut _axis_form = serde_json::Map::new();").unwrap();
    writeln!(out, "    loop {{").unwrap();
    writeln!(
        out,
        "        let _axis_field = match _axis_multipart.next_field().await {{"
    )
    .unwrap();
    writeln!(out, "            Ok(Some(field)) => field,").unwrap();
    writeln!(out, "            Ok(None) => break,").unwrap();
    writeln!(out, "            Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart body: {{error}}\")),").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(
        out,
        "        let Some(_axis_name) = _axis_field.name().map(str::to_owned) else {{ continue }};"
    )
    .unwrap();
    writeln!(out, "        if _axis_form.contains_key(&_axis_name) {{ return api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!(\"duplicate multipart field: {{_axis_name}}\")); }}").unwrap();
    writeln!(
        out,
        "        let _axis_value = match _axis_name.as_str() {{"
    )
    .unwrap();
    for field in &body.fields {
        let field_name = serde_json::to_string(&field.name).expect("body field is serializable");
        let ty = match &field.ty {
            TypeExpr::Maybe(inner) => inner.as_ref(),
            other => other,
        };
        writeln!(out, "            {field_name} => {{").unwrap();
        match ty {
            TypeExpr::Blob => {
                writeln!(out, "                let bytes = match _axis_field.bytes().await {{ Ok(bytes) => bytes, Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart file: {{error}}\")) }};").unwrap();
                writeln!(out, "                serde_json::json!(bytes.to_vec())").unwrap();
            }
            TypeExpr::Int { .. } => {
                writeln!(out, "                let text = match _axis_field.text().await {{ Ok(text) => text, Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart field: {{error}}\")) }};").unwrap();
                writeln!(out, "                match text.parse::<i64>() {{ Ok(value) => serde_json::json!(value), Err(_) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{}: expected an integer\") }}", field.name).unwrap();
            }
            TypeExpr::Bool => {
                writeln!(out, "                let text = match _axis_field.text().await {{ Ok(text) => text, Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart field: {{error}}\")) }};").unwrap();
                writeln!(out, "                match text.parse::<bool>() {{ Ok(value) => serde_json::json!(value), Err(_) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{}: expected true or false\") }}", field.name).unwrap();
            }
            TypeExpr::Json | TypeExpr::List(_) | TypeExpr::Map(_, _) => {
                writeln!(out, "                let text = match _axis_field.text().await {{ Ok(text) => text, Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart field: {{error}}\")) }};").unwrap();
                writeln!(out, "                match serde_json::from_str(&text) {{ Ok(value) => value, Err(_) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, \"{}: expected valid JSON\") }}", field.name).unwrap();
            }
            _ => {
                writeln!(out, "                match _axis_field.text().await {{ Ok(text) => serde_json::Value::String(text), Err(error) => return api_error(StatusCode::BAD_REQUEST, &format!(\"invalid multipart field: {{error}}\")) }}").unwrap();
            }
        }
        writeln!(out, "            }}").unwrap();
    }
    writeln!(out, "            _ => return api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!(\"unexpected multipart field: {{_axis_name}}\")),").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        _axis_form.insert(_axis_name, _axis_value);").unwrap();
    writeln!(out, "    }}").unwrap();
    let mutability = if body
        .fields
        .iter()
        .any(|field| matches!(&field.ty, TypeExpr::String(_) | TypeExpr::Text))
    {
        "mut "
    } else {
        ""
    };
    writeln!(out, "    let {mutability}body: {body_name} = match serde_json::from_value(serde_json::Value::Object(_axis_form)) {{").unwrap();
    writeln!(out, "        Ok(body) => body,").unwrap();
    writeln!(out, "        Err(error) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!(\"invalid multipart body: {{error}}\")),").unwrap();
    writeln!(out, "    }};").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::compile_source;

    #[test]
    fn test_generate_rust_project() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.cargo_toml.contains("[package]"));
        assert!(project.cargo_toml.contains("axum"));
        assert!(project.main_rs.contains("async fn handle_get_user"));
        assert!(project.main_rs.contains("healthz"));
        assert!(project.main_rs.contains("readyz"));
        assert!(project.main_rs.contains("metrics"));
        assert!(project.main_rs.contains("#[tokio::main]"));
        assert!(!project.sql_schema.is_empty());
    }

    #[test]
    fn test_generate_with_body() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW create_user post /users
  AUTH session
  BODY CreateUser
    name STRING 100 REQUIRED
  INSERT users
    name body.name
  AS user
  RETURN 201 user
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("CreateUserBody"));
        assert!(project.main_rs.contains("Json(body)"));
    }

    #[test]
    fn test_generate_with_params() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  role STRING 20

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW list_users get /users
  REALM api
  AUTH session
  PARAM role STRING 20
  LET users
    QUERY users
      FILTER role EQ query.role
      PAGE_SIZE 20
  RETURN 200 users
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("ListUsersQueryParams"));
        assert!(project.main_rs.contains("Query(query)"));
        assert!(project.main_rs.contains("role: Option<String>"));
    }

    #[test]
    fn test_unreferenced_params_excluded() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW list_users get /users
  AUTH session
  PARAM unused_filter STRING 50
  LET users
    QUERY users
  RETURN 200 users
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(!project.main_rs.contains("ListUsersQueryParams"));
        assert!(!project.main_rs.contains("unused_filter"));
    }

    #[test]
    fn test_generate_booking_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("handle_get_booking"));
        assert!(project.main_rs.contains("handle_list_bookings"));
        assert!(project.main_rs.contains("handle_create_booking"));
        assert!(project.main_rs.contains("build_router"));
    }

    #[test]
    fn test_generate_surface_routes() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

SURFACE public v1
  BASE_PATH /api/v1
  ROUTE GET /users/:id -> get_user
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("/api/v1/users/{id}"));
    }

    #[test]
    fn test_cargo_toml_deps() {
        let auth = AuthUsage::default();
        let toml = generate_cargo_toml(false, false, false, false, false, &auth);
        assert!(toml.contains("axum"));
        assert!(toml.contains("sqlx"));
        assert!(toml.contains("tokio"));
        assert!(toml.contains("prometheus"));
        assert!(toml.contains("tracing"));
        assert!(toml.contains("uuid"));
        assert!(!toml.contains("tokio-stream"));
        assert!(!toml.contains("jsonwebtoken"));
        assert!(!toml.contains("hmac"));
    }

    #[test]
    fn test_cargo_toml_with_streams() {
        let auth = AuthUsage::default();
        let toml = generate_cargo_toml(true, false, false, false, false, &auth);
        assert!(toml.contains("tokio-stream"));
        assert!(toml.contains("futures"));
        assert!(toml.contains("features = [\"ws\"]"));
    }

    #[test]
    fn test_cargo_toml_with_auth() {
        let auth = AuthUsage {
            session: true,
            bearer: true,
            api_key: true,
            webhook: true,
            needs_claims: true,
        };
        let toml = generate_cargo_toml(false, false, false, false, false, &auth);
        assert!(toml.contains("jsonwebtoken"));
        assert!(toml.contains("hmac"));
        assert!(toml.contains("sha2"));
        assert!(toml.contains("hex"));
    }

    #[test]
    fn test_generate_with_stream() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW get_user get /users/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

STREAM updates ws /ws/updates
  AUTH session
  EVENT user_changed
    user_id UUID
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("handle_stream_updates"));
        assert!(project.main_rs.contains("/ws/updates"));
        assert!(project.cargo_toml.contains("tokio-stream"));
    }

    #[test]
    fn test_generate_full_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let project = generate(&program);

        assert!(project.main_rs.contains("handle_stream_order_updates"));
        assert!(project.main_rs.contains("/ws/orders"));
    }

    #[test]
    fn test_auth_codegen_full() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let project = generate(&program);
        assert!(project.main_rs.contains("AuthClaims"));
        assert!(project.main_rs.contains("extract_session"));
        assert!(!project.main_rs.contains("verify_api_key"));
        assert!(!project.main_rs.contains("verify_webhook_signature"));
        assert!(project.main_rs.contains("jsonwebtoken"));
        assert!(project.main_rs.contains("JWT_SECRET"));
        assert!(project.cargo_toml.contains("jsonwebtoken"));
    }

    #[test]
    fn test_auth_codegen_selective() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH bearer
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

FLOW list_users get /users
  REALM api
  AUTH api_key
  LET users
    FETCH users
    OR 500
  RETURN 200 users
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);
        assert!(project.main_rs.contains("extract_bearer"));
        assert!(project.main_rs.contains("verify_api_key"));
        assert!(project.main_rs.contains("api_key: Option<String>"));
        assert!(!project.main_rs.contains("extract_session"));
        assert!(!project.main_rs.contains("verify_webhook_signature"));
        assert!(project.cargo_toml.contains("jsonwebtoken"));
        assert!(project.cargo_toml.contains("hmac"));
        assert!(!project.cargo_toml.contains("hex"));
    }

    #[test]
    fn test_middleware_layers() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let project = generate(&program);
        assert!(project.main_rs.contains("TraceLayer"));
        assert!(project.main_rs.contains("CorsLayer"));
        assert!(project.main_rs.contains("RequestBodyLimitLayer"));
        assert!(project.main_rs.contains(".timeout("));
    }

    #[test]
    fn test_audit_fixes() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let project = generate(&program);
        let m = &project.main_rs;

        // #50: No json!({ let ... }) blocks — DaysBetween etc use outer block
        assert!(!m.contains("json!({ let"));
        // #51: Ident literals are json strings, not bare variables
        assert!(!m.contains(".clone()\",\n            LiteralValue::Ident"));
        // #52: AuthClaims has serde(flatten) extra field
        assert!(m.contains("#[serde(flatten)]"));
        assert!(m.contains("extra: std::collections::HashMap<String, serde_json::Value>"));
        // #57: Metrics recorded in handlers
        assert!(m.contains("request_counter.with_label_values"));
        assert!(m.contains("request_duration.with_label_values"));
        assert!(m.contains("_start.elapsed().as_secs_f64()"));
        // #58: Per-source pools (replaced transactions)
        assert!(m.contains("db_users"));
        assert!(m.contains("db_listings"));
        assert!(m.contains("db_bookings"));
        // #59: row_to_json handles Date/Timestamp/Decimal
        assert!(m.contains("chrono::NaiveDate"));
        assert!(m.contains("chrono::DateTime<chrono::Utc>"));
        assert!(m.contains("rust_decimal::Decimal"));
        // #60: db_try! macro, no .unwrap() on DB ops
        assert!(m.contains("macro_rules! db_try"));
        assert!(m.contains("db_try!(sqlx::query"));
        assert!(!m.contains(".fetch_one(&state.db).await.unwrap()"));
        assert!(!m.contains(".execute(&state.db).await.unwrap()"));
        // #61: No double into_response
        assert!(!m.contains(".into_response()).into_response()"));
        // #55/#56: Rate limiters and cache in AppState
        assert!(m.contains("rate_limiters"));
        assert!(m.contains("response_cache"));
    }

    #[test]
    fn test_string_sanitization() {
        let input = r#"SHAPE Post
  id UUID PK AUTO
  title STRING 200 REQUIRED
  content TEXT

SOURCE posts POSTGRES
  SHAPE Post

FLOW create_post post /posts
  AUTH none
  BODY PostBody
    title STRING 200 REQUIRED
    content TEXT
  INSERT posts
    title body.title
    content body.content
  AS post
  RETURN 201 post
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);
        let m = &project.main_rs;
        assert!(m.contains("Json(mut body)"));
        assert!(m.contains("body.title = body.title.trim()"));
        assert!(m.contains("is_control()"));
    }

    #[test]
    fn test_messenger_primitives_generate_atomic_rust() {
        let input = include_str!("../../examples/messenger-primitives.axis");
        let program = compile_source(input).unwrap();
        let project = generate(&program);
        let generated = &project.main_rs;

        assert!(project.cargo_toml.contains("sha2 = \"0.10\""));
        assert!(project.cargo_toml.contains("hex = \"0.4\""));
        assert!(generated.contains("let mut _axis_tx = match state.db_deliveries.begin().await"));
        assert!(
            generated.contains("ON CONFLICT (flow_name, scope_key, idempotency_key) DO NOTHING")
        );
        assert!(generated.contains(".fetch_one(&mut *_axis_tx).await"));
        assert!(generated.contains("QueryBuilder::<sqlx::Sqlite>"));
        assert!(generated.contains("recipient.clone()).as_str()"));
        assert!(generated.contains("_axis_tx.commit().await"));
        assert!(generated.contains("idempotency-replayed"));
        assert!(!generated.contains("\"upserted\": true"));
    }

    #[test]
    fn test_upload_codegen_is_executable_for_local_and_s3_storage() {
        let input = r#"STORAGE local_avatars
  BACKEND local
  BUCKET uploads
  PREFIX avatars
  ACCESS public
  MAX_SIZE 5242880
  TYPES image/jpeg image/png

STORAGE cloud_avatars
  BACKEND s3
  BUCKET "production-avatars"
  PREFIX originals
  ACCESS private
  MAX_SIZE 5242880
  TYPES image/jpeg image/png

FLOW upload_avatar post /avatars
  BODY MULTIPART AvatarUpload
    file BLOB REQUIRED
  UPLOAD body.file -> local_avatars AS local_url
  UPLOAD body.file -> cloud_avatars AS cloud_url
  RETURN 201
    local_url local_url
    cloud_url cloud_url
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.cargo_toml.contains("features = [\"multipart\"]"));
        assert!(project.cargo_toml.contains("object_store"));
        assert!(project.main_rs.contains("Multipart::from_request"));
        assert!(project.main_rs.contains("tokio::fs::write"));
        assert!(project.main_rs.contains("ObjectStore::put_opts"));
        assert!(project.main_rs.contains("/files/local_avatars/"));
        assert!(!project.main_rs.contains("TODO"));
    }
}
