use std::collections::HashMap;
use std::pin::Pin;
use std::future::Future;

use redis::AsyncCommands;
use serde_json::Value;

use super::adapter::*;

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct RedisPort {
    conn: redis::aio::ConnectionManager,
    prefix: String,
}

impl RedisPort {
    pub async fn connect(url: &str, prefix: &str) -> Result<Self, AdapterError> {
        let client = redis::Client::open(url)
            .map_err(|e| AdapterError::ConnectionFailed(e.to_string()))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| AdapterError::ConnectionFailed(e.to_string()))?;
        Ok(RedisPort {
            conn,
            prefix: prefix.to_string(),
        })
    }

    fn key(&self, source: &str, id: &str) -> String {
        if self.prefix.is_empty() {
            format!("{source}:{id}")
        } else {
            format!("{}:{source}:{id}", self.prefix)
        }
    }

    #[allow(dead_code)]
    fn pattern(&self, source: &str) -> String {
        if self.prefix.is_empty() {
            format!("{source}:*")
        } else {
            format!("{}:{source}:*", self.prefix)
        }
    }

    fn extract_id(filters: &[FilterParam]) -> Option<&str> {
        filters.iter().find_map(|f| {
            if (f.field == "id" || f.field == "key") && matches!(f.op, FilterOp::Eq) {
                f.value.as_str()
            } else {
                None
            }
        })
    }
}

impl SourceAdapter for RedisPort {
    fn source_type(&self) -> &str {
        "redis"
    }

    fn fetch(&self, req: FetchRequest) -> BoxFut<'_, Result<Option<Value>, AdapterError>> {
        Box::pin(async move {
            let id = Self::extract_id(&req.filters)
                .ok_or_else(|| AdapterError::OperationFailed("FETCH requires id or key filter".into()))?;
            let key = self.key(&req.source, id);
            let mut conn = self.conn.clone();
            let raw: Option<String> = conn.get(&key).await
                .map_err(|e| AdapterError::OperationFailed(e.to_string()))?;
            match raw {
                Some(s) => {
                    let val: Value = serde_json::from_str(&s)
                        .unwrap_or(Value::String(s));
                    Ok(Some(val))
                }
                None => Ok(None),
            }
        })
    }

    fn insert(&self, req: InsertRequest) -> BoxFut<'_, Result<Value, AdapterError>> {
        Box::pin(async move {
            let id = req.fields.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let key = self.key(&req.source, &id);
            let mut record = req.fields.clone();
            record.insert("id".into(), Value::String(id));
            let json = serde_json::to_string(&record)
                .map_err(|e| AdapterError::OperationFailed(e.to_string()))?;
            let mut conn = self.conn.clone();
            let _: () = conn.set(&key, &json).await
                .map_err(|e| AdapterError::OperationFailed(e.to_string()))?;
            Ok(serde_json::json!(record))
        })
    }

    fn delete(&self, req: DeleteRequest) -> BoxFut<'_, Result<u64, AdapterError>> {
        Box::pin(async move {
            let id = Self::extract_id(&req.filters)
                .ok_or_else(|| AdapterError::OperationFailed("DELETE requires id or key filter".into()))?;
            let key = self.key(&req.source, id);
            let mut conn = self.conn.clone();
            let deleted: u64 = conn.del(&key).await
                .map_err(|e| AdapterError::OperationFailed(e.to_string()))?;
            Ok(deleted)
        })
    }
}

pub fn redis_factory() -> AdapterFactory {
    Box::new(|config: &HashMap<String, String>| {
        let url = config.get("url").cloned().unwrap_or_else(|| "redis://127.0.0.1/".into());
        let prefix = config.get("prefix").cloned().unwrap_or_default();
        Box::pin(async move {
            let adapter = RedisPort::connect(&url, &prefix).await?;
            Ok(Box::new(adapter) as Box<dyn SourceAdapter>)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(prefix: &str, source: &str, id: &str) -> String {
        if prefix.is_empty() {
            format!("{source}:{id}")
        } else {
            format!("{prefix}:{source}:{id}")
        }
    }

    #[test]
    fn test_key_format() {
        assert_eq!(test_key("app", "users", "123"), "app:users:123");
        assert_eq!(test_key("", "users", "123"), "users:123");
    }

    #[test]
    fn test_extract_id() {
        let filters = vec![FilterParam {
            field: "id".into(),
            op: FilterOp::Eq,
            value: serde_json::json!("abc"),
        }];
        assert_eq!(RedisPort::extract_id(&filters), Some("abc"));

        let filters = vec![FilterParam {
            field: "key".into(),
            op: FilterOp::Eq,
            value: serde_json::json!("xyz"),
        }];
        assert_eq!(RedisPort::extract_id(&filters), Some("xyz"));

        let empty: Vec<FilterParam> = vec![];
        assert_eq!(RedisPort::extract_id(&empty), None);
    }

    #[test]
    fn test_factory_created() {
        let factory = redis_factory();
        let _ = factory;
    }
}
