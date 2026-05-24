use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug)]
pub enum AdapterError {
    NotFound,
    ConnectionFailed(String),
    OperationFailed(String),
    Unsupported(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::NotFound => write!(f, "not found"),
            AdapterError::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            AdapterError::OperationFailed(msg) => write!(f, "operation failed: {msg}"),
            AdapterError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

impl std::error::Error for AdapterError {}

pub struct FetchRequest {
    pub source: String,
    pub filters: Vec<FilterParam>,
}

pub struct QueryRequest {
    pub source: String,
    pub filters: Vec<FilterParam>,
    pub sorts: Vec<SortParam>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub struct InsertRequest {
    pub source: String,
    pub fields: HashMap<String, Value>,
}

pub struct UpdateRequest {
    pub source: String,
    pub filters: Vec<FilterParam>,
    pub fields: HashMap<String, Value>,
}

pub struct DeleteRequest {
    pub source: String,
    pub filters: Vec<FilterParam>,
}

pub struct FilterParam {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
}

pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    Like,
    Contains,
    StartsWith,
    Between,
}

pub struct SortParam {
    pub field: String,
    pub ascending: bool,
}

pub struct QueryResult {
    pub items: Vec<Value>,
    pub total: Option<i64>,
}

pub trait SourceAdapter: Send + Sync {
    fn source_type(&self) -> &str;

    fn fetch(&self, _req: FetchRequest) -> BoxFut<'_, Result<Option<Value>, AdapterError>> {
        let stype = self.source_type().to_string();
        Box::pin(async move { Err(AdapterError::Unsupported(format!("FETCH not supported for {stype}"))) })
    }

    fn query(&self, _req: QueryRequest) -> BoxFut<'_, Result<QueryResult, AdapterError>> {
        let stype = self.source_type().to_string();
        Box::pin(async move { Err(AdapterError::Unsupported(format!("QUERY not supported for {stype}"))) })
    }

    fn insert(&self, _req: InsertRequest) -> BoxFut<'_, Result<Value, AdapterError>> {
        let stype = self.source_type().to_string();
        Box::pin(async move { Err(AdapterError::Unsupported(format!("INSERT not supported for {stype}"))) })
    }

    fn update(&self, _req: UpdateRequest) -> BoxFut<'_, Result<u64, AdapterError>> {
        let stype = self.source_type().to_string();
        Box::pin(async move { Err(AdapterError::Unsupported(format!("UPDATE not supported for {stype}"))) })
    }

    fn delete(&self, _req: DeleteRequest) -> BoxFut<'_, Result<u64, AdapterError>> {
        let stype = self.source_type().to_string();
        Box::pin(async move { Err(AdapterError::Unsupported(format!("DELETE not supported for {stype}"))) })
    }
}

pub type AdapterFactory =
    Box<dyn Fn(&HashMap<String, String>) -> BoxFut<'static, Result<Box<dyn SourceAdapter>, AdapterError>> + Send + Sync>;

pub struct AdapterRegistry {
    factories: HashMap<String, AdapterFactory>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register(&mut self, source_type: &str, factory: AdapterFactory) {
        self.factories.insert(source_type.to_lowercase(), factory);
    }

    pub fn has(&self, source_type: &str) -> bool {
        self.factories.contains_key(&source_type.to_lowercase())
    }

    pub async fn create(
        &self,
        source_type: &str,
        config: &HashMap<String, String>,
    ) -> Result<Box<dyn SourceAdapter>, AdapterError> {
        let factory = self
            .factories
            .get(&source_type.to_lowercase())
            .ok_or_else(|| AdapterError::Unsupported(format!("no adapter for {source_type}")))?;
        factory(config).await
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRedis {
        data: std::sync::Mutex<HashMap<String, Value>>,
    }

    impl MockRedis {
        fn new() -> Self {
            MockRedis {
                data: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    impl SourceAdapter for MockRedis {
        fn source_type(&self) -> &str {
            "redis"
        }

        fn fetch(&self, req: FetchRequest) -> BoxFut<'_, Result<Option<Value>, AdapterError>> {
            Box::pin(async move {
                let key = req.filters.first()
                    .map(|f| f.value.as_str().unwrap_or_default().to_string())
                    .unwrap_or_default();
                let data = self.data.lock().unwrap();
                Ok(data.get(&key).cloned())
            })
        }

        fn insert(&self, req: InsertRequest) -> BoxFut<'_, Result<Value, AdapterError>> {
            Box::pin(async move {
                let key = req.fields.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("auto")
                    .to_string();
                let val = serde_json::json!(req.fields);
                self.data.lock().unwrap().insert(key, val.clone());
                Ok(val)
            })
        }

        fn delete(&self, req: DeleteRequest) -> BoxFut<'_, Result<u64, AdapterError>> {
            Box::pin(async move {
                let key = req.filters.first()
                    .map(|f| f.value.as_str().unwrap_or_default().to_string())
                    .unwrap_or_default();
                let removed = self.data.lock().unwrap().remove(&key).is_some();
                Ok(if removed { 1 } else { 0 })
            })
        }
    }

    #[tokio::test]
    async fn test_adapter_crud() {
        let adapter = MockRedis::new();
        assert_eq!(adapter.source_type(), "redis");

        let result = adapter.insert(InsertRequest {
            source: "cache".into(),
            fields: HashMap::from([
                ("id".into(), serde_json::json!("key1")),
                ("value".into(), serde_json::json!(42)),
            ]),
        }).await.unwrap();
        assert_eq!(result["id"], "key1");

        let fetched = adapter.fetch(FetchRequest {
            source: "cache".into(),
            filters: vec![FilterParam {
                field: "id".into(),
                op: FilterOp::Eq,
                value: serde_json::json!("key1"),
            }],
        }).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap()["value"], 42);

        let query_result = adapter.query(QueryRequest {
            source: "cache".into(),
            filters: vec![],
            sorts: vec![],
            limit: None,
            offset: None,
        }).await;
        assert!(query_result.is_err());

        let deleted = adapter.delete(DeleteRequest {
            source: "cache".into(),
            filters: vec![FilterParam {
                field: "id".into(),
                op: FilterOp::Eq,
                value: serde_json::json!("key1"),
            }],
        }).await.unwrap();
        assert_eq!(deleted, 1);
    }

    #[tokio::test]
    async fn test_adapter_registry() {
        let mut registry = AdapterRegistry::new();
        registry.register(
            "redis",
            Box::new(|_config: &HashMap<String, String>| {
                Box::pin(async move {
                    Ok(Box::new(MockRedis::new()) as Box<dyn SourceAdapter>)
                })
            }),
        );

        assert!(registry.has("redis"));
        assert!(!registry.has("elasticsearch"));

        let adapter = registry.create("redis", &HashMap::new()).await.unwrap();
        assert_eq!(adapter.source_type(), "redis");
    }

    #[tokio::test]
    async fn test_unsupported_operations() {
        struct MinimalAdapter;
        impl SourceAdapter for MinimalAdapter {
            fn source_type(&self) -> &str { "minimal" }
        }

        let adapter = MinimalAdapter;
        assert!(adapter.fetch(FetchRequest { source: "x".into(), filters: vec![] }).await.is_err());
        assert!(adapter.query(QueryRequest { source: "x".into(), filters: vec![], sorts: vec![], limit: None, offset: None }).await.is_err());
        assert!(adapter.insert(InsertRequest { source: "x".into(), fields: HashMap::new() }).await.is_err());
        assert!(adapter.update(UpdateRequest { source: "x".into(), filters: vec![], fields: HashMap::new() }).await.is_err());
        assert!(adapter.delete(DeleteRequest { source: "x".into(), filters: vec![] }).await.is_err());
    }
}
