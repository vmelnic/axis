use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct AxisConfig {
    pub axis: AxisMeta,
    #[serde(default)]
    pub sources: HashMap<String, SourceConfig>,
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,
    #[serde(default)]
    pub vault: Option<VaultConfig>,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub effects: HashMap<String, EffectConfig>,
    #[serde(default)]
    pub server: Option<ServerConfig>,
}

#[derive(Debug, Deserialize)]
pub struct AxisMeta {
    pub version: String,
    #[serde(default)]
    pub manifest: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SourceConfig {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    #[serde(default)]
    pub statement_timeout: Option<String>,
}

fn default_pool_size() -> u32 {
    10
}

#[derive(Debug, Deserialize)]
pub struct ServiceConfig {
    pub endpoint: String,
}

#[derive(Debug, Deserialize)]
pub struct VaultConfig {
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub session: Option<SessionAuthConfig>,
    #[serde(default)]
    pub api_key: Option<ApiKeyAuthConfig>,
}

#[derive(Debug, Deserialize)]
pub struct SessionAuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub jwks_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyAuthConfig {
    pub header: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct EffectConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub credentials: Option<String>,
    #[serde(default)]
    pub poll_interval: Option<String>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub backoff: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub graceful_shutdown: Option<String>,
    #[serde(default)]
    pub request_timeout: Option<String>,
    #[serde(default)]
    pub cors: Option<CorsConfig>,
}

fn default_port() -> u16 {
    8080
}

#[derive(Debug, Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub origins: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub headers: Vec<String>,
    #[serde(default)]
    pub max_age: Option<u32>,
}

#[derive(Debug)]
pub struct ConfigError {
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub fn load_config(path: &Path) -> Result<AxisConfig, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|e| ConfigError {
        message: format!("cannot read {}: {e}", path.display()),
    })?;
    parse_config(&content)
}

pub fn parse_config(content: &str) -> Result<AxisConfig, ConfigError> {
    let config: AxisConfig = serde_yaml::from_str(content).map_err(|e| ConfigError {
        message: format!("invalid config: {e}"),
    })?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &AxisConfig) -> Result<(), ConfigError> {
    if config.axis.version.is_empty() {
        return Err(ConfigError {
            message: "axis.version is required".into(),
        });
    }

    let valid_types = [
        "postgres",
        "mysql",
        "sqlite",
        "redis",
        "elasticsearch",
        "dynamodb",
    ];
    for (name, source) in &config.sources {
        if !valid_types.contains(&source.source_type.as_str()) {
            return Err(ConfigError {
                message: format!(
                    "source '{}': unknown type '{}', expected one of: {}",
                    name,
                    source.source_type,
                    valid_types.join(", ")
                ),
            });
        }
        if source.pool_size == 0 {
            return Err(ConfigError {
                message: format!("source '{}': pool_size must be > 0", name),
            });
        }
    }

    if let Some(ref vault) = config.vault {
        let valid_providers = ["env", "aws_secrets_manager", "hashicorp_vault"];
        if !valid_providers.contains(&vault.provider.as_str()) {
            return Err(ConfigError {
                message: format!(
                    "vault: unknown provider '{}', expected one of: {}",
                    vault.provider,
                    valid_providers.join(", ")
                ),
            });
        }
    }

    if let Some(ref auth) = config.auth {
        if let Some(ref session) = auth.session {
            let valid_auth_types = ["jwt", "opaque"];
            if !valid_auth_types.contains(&session.auth_type.as_str()) {
                return Err(ConfigError {
                    message: format!(
                        "auth.session: unknown type '{}', expected one of: {}",
                        session.auth_type,
                        valid_auth_types.join(", ")
                    ),
                });
            }
        }
    }

    if let Some(ref server) = config.server {
        if server.port == 0 {
            return Err(ConfigError {
                message: "server.port must be > 0".into(),
            });
        }
    }

    Ok(())
}

pub fn validate_config_against_program(
    config: &AxisConfig,
    program: &crate::ast::Program,
) -> Vec<ConfigError> {
    use crate::ast::Construct;
    let mut errors = Vec::new();

    for construct in &program.constructs {
        if let Construct::Source(source) = construct {
            if !config.sources.contains_key(&source.name) {
                errors.push(ConfigError {
                    message: format!(
                        "SOURCE '{}' declared in program but not configured in axis.yaml",
                        source.name
                    ),
                });
            }
        }
        if let Construct::Service(service) = construct {
            if !config.services.contains_key(&service.name) {
                errors.push(ConfigError {
                    message: format!(
                        "SERVICE '{}' declared in program but not configured in axis.yaml",
                        service.name
                    ),
                });
            }
        }
    }

    for (name, source_cfg) in &config.sources {
        let has_source = program
            .constructs
            .iter()
            .any(|c| matches!(c, Construct::Source(s) if s.name == *name));
        if !has_source {
            errors.push(ConfigError {
                message: format!(
                    "source '{}' configured in axis.yaml but not declared as SOURCE in program",
                    name
                ),
            });
        }
        let expected_type = program.constructs.iter().find_map(|c| {
            if let Construct::Source(s) = c {
                if s.name == *name {
                    return Some(format!("{:?}", s.source_type).to_lowercase());
                }
            }
            None
        });
        if let Some(expected) = expected_type {
            if source_cfg.source_type != expected {
                errors.push(ConfigError {
                    message: format!(
                        "source '{}': config type '{}' does not match SOURCE type '{}'",
                        name, source_cfg.source_type, expected
                    ),
                });
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let yaml = r#"
axis:
  version: "0.2"
"#;
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.axis.version, "0.2");
        assert!(config.sources.is_empty());
    }

    #[test]
    fn test_parse_full_config() {
        let yaml = r#"
axis:
  version: "0.2"
  manifest: "sha256:abc123"

sources:
  bookings:
    type: postgres
    url: "${DATABASE_URL}"
    pool_size: 20
    statement_timeout: 5s
  cache:
    type: redis
    url: "${REDIS_URL}"

services:
  payments:
    endpoint: "https://api.stripe.com/v1"

vault:
  provider: env

auth:
  session:
    type: jwt
    issuer: "${AUTH_ISSUER}"
  api_key:
    header: "X-API-Key"
    source: api_keys

effects:
  email:
    provider: sendgrid
    api_key: "${SENDGRID_KEY}"
  outbox:
    poll_interval: 1s
    max_retries: 5
    backoff: exponential

server:
  port: 8080
  graceful_shutdown: 30s
  request_timeout: 30s
  cors:
    origins: ["https://app.example.com"]
    methods: ["GET", "POST"]
    headers: ["Authorization"]
    max_age: 3600
"#;
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.axis.version, "0.2");
        assert_eq!(config.axis.manifest.as_deref(), Some("sha256:abc123"));
        assert_eq!(config.sources.len(), 2);
        assert_eq!(config.sources["bookings"].pool_size, 20);
        assert_eq!(config.services.len(), 1);
        assert_eq!(config.vault.as_ref().unwrap().provider, "env");
        assert_eq!(config.server.as_ref().unwrap().port, 8080);
        let cors = config.server.as_ref().unwrap().cors.as_ref().unwrap();
        assert_eq!(cors.origins.len(), 1);
        assert_eq!(cors.max_age, Some(3600));
    }

    #[test]
    fn test_invalid_source_type() {
        let yaml = r#"
axis:
  version: "0.2"
sources:
  db:
    type: mongodb
    url: "localhost"
"#;
        let err = parse_config(yaml).unwrap_err();
        assert!(err.message.contains("unknown type 'mongodb'"));
    }

    #[test]
    fn test_invalid_vault_provider() {
        let yaml = r#"
axis:
  version: "0.2"
vault:
  provider: unknown
"#;
        let err = parse_config(yaml).unwrap_err();
        assert!(err.message.contains("unknown provider 'unknown'"));
    }

    #[test]
    fn test_default_pool_size() {
        let yaml = r#"
axis:
  version: "0.2"
sources:
  db:
    type: postgres
    url: "localhost"
"#;
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.sources["db"].pool_size, 10);
    }

    #[test]
    fn test_cross_validate_missing_source() {
        let yaml = r#"
axis:
  version: "0.2"
"#;
        let config = parse_config(yaml).unwrap();
        let program = crate::ast::Program {
            constructs: vec![crate::ast::Construct::Source(crate::ast::SourceDef {
                name: "users".into(),
                source_type: crate::ast::SourceType::Postgres,
                shape: "User".into(),
                indexes: vec![],
                ttl: None,
                span: crate::token::Span {
                    offset: 0,
                    len: 0,
                    line: 1,
                    col: 1,
                },
            })],
        };
        let errors = validate_config_against_program(&config, &program);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not configured in axis.yaml"));
    }
}
