use std::fmt::Write;

use serde::Serialize;

use crate::ast::*;

#[derive(Debug, Serialize)]
pub struct DeployManifest {
    pub dockerfile: String,
    pub compose: String,
    pub kubernetes: String,
    pub env_template: String,
    pub infrastructure: InfraRequirements,
}

#[derive(Debug, Serialize)]
pub struct InfraRequirements {
    pub databases: Vec<DatabaseReq>,
    pub services: Vec<ServiceReq>,
    pub streams: Vec<StreamReq>,
    pub port: u16,
}

#[derive(Debug, Serialize)]
pub struct DatabaseReq {
    pub name: String,
    pub engine: String,
}

#[derive(Debug, Serialize)]
pub struct ServiceReq {
    pub name: String,
    pub endpoint: String,
}

#[derive(Debug, Serialize)]
pub struct StreamReq {
    pub name: String,
    pub transport: String,
}

pub fn generate_deploy(program: &Program) -> DeployManifest {
    let infra = collect_infra(program);

    DeployManifest {
        dockerfile: generate_dockerfile(),
        compose: generate_compose(&infra),
        kubernetes: generate_k8s(&infra),
        env_template: generate_env(&infra),
        infrastructure: infra,
    }
}

fn collect_infra(program: &Program) -> InfraRequirements {
    let mut databases = Vec::new();
    let mut services = Vec::new();
    let mut streams = Vec::new();

    for construct in &program.constructs {
        match construct {
            Construct::Source(s) => {
                let engine = match s.source_type {
                    SourceType::Postgres => "postgres",
                    SourceType::Mysql => "mysql",
                    SourceType::Sqlite => "sqlite",
                    SourceType::Redis => "redis",
                    SourceType::Elasticsearch => "elasticsearch",
                    SourceType::Dynamodb => "dynamodb",
                };
                if !databases.iter().any(|d: &DatabaseReq| d.engine == engine) {
                    databases.push(DatabaseReq {
                        name: s.name.clone(),
                        engine: engine.into(),
                    });
                }
            }
            Construct::Service(svc) => {
                services.push(ServiceReq {
                    name: svc.name.clone(),
                    endpoint: svc.endpoint.clone(),
                });
            }
            Construct::Stream(st) => {
                let transport = match st.transport {
                    StreamTransport::WebSocket => "websocket",
                    StreamTransport::Sse => "sse",
                };
                streams.push(StreamReq {
                    name: st.name.clone(),
                    transport: transport.into(),
                });
            }
            _ => {}
        }
    }

    InfraRequirements {
        databases,
        services,
        streams,
        port: 8080,
    }
}

fn generate_dockerfile() -> String {
    r#"FROM rust:1.83-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src/ src/
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/axis-server /usr/local/bin/axis-server
WORKDIR /app
EXPOSE 8080
CMD ["axis-server"]
"#
    .into()
}

fn generate_compose(infra: &InfraRequirements) -> String {
    let mut out = String::new();
    writeln!(out, "services:").unwrap();
    writeln!(out, "  app:").unwrap();
    writeln!(out, "    build: .").unwrap();
    writeln!(out, "    ports:").unwrap();
    writeln!(out, "      - \"{}:{}\"", infra.port, infra.port).unwrap();
    writeln!(out, "    environment:").unwrap();
    writeln!(out, "      - PORT={}", infra.port).unwrap();
    writeln!(
        out,
        "      - JWT_SECRET=${{JWT_SECRET:-change-me-in-production}}"
    )
    .unwrap();
    writeln!(out, "      - RUN_MIGRATIONS=1").unwrap();

    let mut depends = Vec::new();

    for db in &infra.databases {
        match db.engine.as_str() {
            "postgres" => {
                writeln!(
                    out,
                    "      - DATABASE_URL=postgres://axis:axis@postgres:5432/axis"
                )
                .unwrap();
                depends.push("postgres");
            }
            "redis" => {
                writeln!(out, "      - REDIS_URL=redis://redis:6379").unwrap();
                depends.push("redis");
            }
            "mysql" => {
                writeln!(
                    out,
                    "      - DATABASE_URL=mysql://axis:axis@mysql:3306/axis"
                )
                .unwrap();
                depends.push("mysql");
            }
            "elasticsearch" => {
                writeln!(out, "      - ELASTICSEARCH_URL=http://elasticsearch:9200").unwrap();
                depends.push("elasticsearch");
            }
            _ => {}
        }
    }

    for svc in &infra.services {
        let key = svc.name.to_uppercase();
        writeln!(out, "      - {key}_URL={}", svc.endpoint).unwrap();
    }

    if !depends.is_empty() {
        writeln!(out, "    depends_on:").unwrap();
        for d in &depends {
            if *d == "postgres" {
                writeln!(out, "      {d}:").unwrap();
                writeln!(out, "        condition: service_healthy").unwrap();
            } else {
                writeln!(out, "      - {d}").unwrap();
            }
        }
    }
    writeln!(out).unwrap();

    for db in &infra.databases {
        match db.engine.as_str() {
            "postgres" => {
                writeln!(out, "  postgres:").unwrap();
                writeln!(out, "    image: postgres:16-alpine").unwrap();
                writeln!(out, "    environment:").unwrap();
                writeln!(out, "      - POSTGRES_USER=axis").unwrap();
                writeln!(out, "      - POSTGRES_PASSWORD=axis").unwrap();
                writeln!(out, "      - POSTGRES_DB=axis").unwrap();
                writeln!(out, "    ports:").unwrap();
                writeln!(out, "      - \"5432:5432\"").unwrap();
                writeln!(out, "    volumes:").unwrap();
                writeln!(out, "      - pgdata:/var/lib/postgresql/data").unwrap();
                writeln!(out, "    healthcheck:").unwrap();
                writeln!(out, "      test: [\"CMD-SHELL\", \"pg_isready -U axis\"]").unwrap();
                writeln!(out, "      interval: 5s").unwrap();
                writeln!(out, "      timeout: 5s").unwrap();
                writeln!(out, "      retries: 5").unwrap();
                writeln!(out).unwrap();
            }
            "redis" => {
                writeln!(out, "  redis:").unwrap();
                writeln!(out, "    image: redis:7-alpine").unwrap();
                writeln!(out, "    ports:").unwrap();
                writeln!(out, "      - \"6379:6379\"").unwrap();
                writeln!(out).unwrap();
            }
            "mysql" => {
                writeln!(out, "  mysql:").unwrap();
                writeln!(out, "    image: mysql:8").unwrap();
                writeln!(out, "    environment:").unwrap();
                writeln!(out, "      - MYSQL_ROOT_PASSWORD=axis").unwrap();
                writeln!(out, "      - MYSQL_DATABASE=axis").unwrap();
                writeln!(out, "      - MYSQL_USER=axis").unwrap();
                writeln!(out, "      - MYSQL_PASSWORD=axis").unwrap();
                writeln!(out, "    ports:").unwrap();
                writeln!(out, "      - \"3306:3306\"").unwrap();
                writeln!(out).unwrap();
            }
            "elasticsearch" => {
                writeln!(out, "  elasticsearch:").unwrap();
                writeln!(out, "    image: elasticsearch:8.11.0").unwrap();
                writeln!(out, "    environment:").unwrap();
                writeln!(out, "      - discovery.type=single-node").unwrap();
                writeln!(out, "      - xpack.security.enabled=false").unwrap();
                writeln!(out, "    ports:").unwrap();
                writeln!(out, "      - \"9200:9200\"").unwrap();
                writeln!(out).unwrap();
            }
            _ => {}
        }
    }

    let has_pg = infra.databases.iter().any(|d| d.engine == "postgres");
    if has_pg {
        writeln!(out, "volumes:").unwrap();
        writeln!(out, "  pgdata:").unwrap();
    }

    out
}

fn generate_k8s(infra: &InfraRequirements) -> String {
    let mut out = String::new();

    writeln!(out, "apiVersion: apps/v1").unwrap();
    writeln!(out, "kind: Deployment").unwrap();
    writeln!(out, "metadata:").unwrap();
    writeln!(out, "  name: axis-app").unwrap();
    writeln!(out, "spec:").unwrap();
    writeln!(out, "  replicas: 2").unwrap();
    writeln!(out, "  selector:").unwrap();
    writeln!(out, "    matchLabels:").unwrap();
    writeln!(out, "      app: axis-app").unwrap();
    writeln!(out, "  template:").unwrap();
    writeln!(out, "    metadata:").unwrap();
    writeln!(out, "      labels:").unwrap();
    writeln!(out, "        app: axis-app").unwrap();
    writeln!(out, "    spec:").unwrap();
    writeln!(out, "      containers:").unwrap();
    writeln!(out, "        - name: axis-app").unwrap();
    writeln!(out, "          image: axis-server:latest").unwrap();
    writeln!(out, "          ports:").unwrap();
    writeln!(out, "            - containerPort: {}", infra.port).unwrap();
    writeln!(out, "          env:").unwrap();
    writeln!(out, "            - name: PORT").unwrap();
    writeln!(out, "              value: \"{}\"", infra.port).unwrap();

    for db in &infra.databases {
        match db.engine.as_str() {
            "postgres" => {
                writeln!(out, "            - name: DATABASE_URL").unwrap();
                writeln!(out, "              valueFrom:").unwrap();
                writeln!(out, "                secretKeyRef:").unwrap();
                writeln!(out, "                  name: axis-secrets").unwrap();
                writeln!(out, "                  key: database-url").unwrap();
            }
            "redis" => {
                writeln!(out, "            - name: REDIS_URL").unwrap();
                writeln!(out, "              valueFrom:").unwrap();
                writeln!(out, "                secretKeyRef:").unwrap();
                writeln!(out, "                  name: axis-secrets").unwrap();
                writeln!(out, "                  key: redis-url").unwrap();
            }
            _ => {}
        }
    }

    writeln!(out, "          readinessProbe:").unwrap();
    writeln!(out, "            httpGet:").unwrap();
    writeln!(out, "              path: /healthz").unwrap();
    writeln!(out, "              port: {}", infra.port).unwrap();
    writeln!(out, "            initialDelaySeconds: 5").unwrap();
    writeln!(out, "            periodSeconds: 10").unwrap();
    writeln!(out, "          livenessProbe:").unwrap();
    writeln!(out, "            httpGet:").unwrap();
    writeln!(out, "              path: /healthz").unwrap();
    writeln!(out, "              port: {}", infra.port).unwrap();
    writeln!(out, "            initialDelaySeconds: 10").unwrap();
    writeln!(out, "            periodSeconds: 30").unwrap();

    writeln!(out, "---").unwrap();
    writeln!(out, "apiVersion: v1").unwrap();
    writeln!(out, "kind: Service").unwrap();
    writeln!(out, "metadata:").unwrap();
    writeln!(out, "  name: axis-app").unwrap();
    writeln!(out, "spec:").unwrap();
    writeln!(out, "  selector:").unwrap();
    writeln!(out, "    app: axis-app").unwrap();
    writeln!(out, "  ports:").unwrap();
    writeln!(out, "    - port: 80").unwrap();
    writeln!(out, "      targetPort: {}", infra.port).unwrap();
    writeln!(out, "  type: ClusterIP").unwrap();

    out
}

fn generate_env(infra: &InfraRequirements) -> String {
    let mut out = String::new();
    writeln!(out, "# Generated by Axis compiler").unwrap();
    writeln!(out, "PORT={}", infra.port).unwrap();
    writeln!(out, "JWT_SECRET=change-me-in-production").unwrap();
    writeln!(out, "RUN_MIGRATIONS=1").unwrap();

    for db in &infra.databases {
        match db.engine.as_str() {
            "postgres" => {
                writeln!(out, "DATABASE_URL=postgres://axis:axis@localhost:5432/axis").unwrap()
            }
            "redis" => writeln!(out, "REDIS_URL=redis://localhost:6379").unwrap(),
            "mysql" => writeln!(out, "DATABASE_URL=mysql://axis:axis@localhost:3306/axis").unwrap(),
            "elasticsearch" => writeln!(out, "ELASTICSEARCH_URL=http://localhost:9200").unwrap(),
            _ => {}
        }
    }

    for svc in &infra.services {
        writeln!(out, "{}_URL={}", svc.name.to_uppercase(), svc.endpoint).unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::compile_source;

    #[test]
    fn test_deploy_basic() {
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
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#;
        let program = compile_source(input).unwrap();
        let deploy = generate_deploy(&program);

        assert!(deploy.dockerfile.contains("FROM rust:"));
        assert!(deploy.dockerfile.contains("EXPOSE 8080"));
        assert!(deploy.compose.contains("postgres:16-alpine"));
        assert!(deploy.compose.contains("DATABASE_URL"));
        assert!(deploy.kubernetes.contains("kind: Deployment"));
        assert!(deploy.kubernetes.contains("kind: Service"));
        assert!(deploy.kubernetes.contains("/healthz"));
        assert!(deploy.env_template.contains("DATABASE_URL"));
    }

    #[test]
    fn test_deploy_with_service() {
        let input = r#"SHAPE Order
  id UUID PK AUTO
  total DECIMAL PRECISION 10 SCALE 2

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders
  CAPABILITY write orders

SERVICE payments
  ENDPOINT stripe
  AUTH bearer VAULT stripe_key
  METHOD charge
    INPUT amount DECIMAL
    OUTPUT id STRING 100
    TIMEOUT 30 s
    RETRY 3 BACKOFF exponential

FLOW charge post /orders/:id/charge
  REALM api
  AUTH session
  LET order
    FETCH orders
      FILTER id EQ path.id
    OR 404
  LET payment
    CALL payments.charge
      amount order.total
    OR 500
  RETURN 200 payment
"#;
        let program = compile_source(input).unwrap();
        let deploy = generate_deploy(&program);

        assert_eq!(deploy.infrastructure.services.len(), 1);
        assert_eq!(deploy.infrastructure.services[0].name, "payments");
        assert!(deploy.compose.contains("PAYMENTS_URL"));
        assert!(deploy.env_template.contains("PAYMENTS_URL"));
    }

    #[test]
    fn test_deploy_with_stream() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id

STREAM updates ws /ws/updates
  EVENT user_online
    user_id UUID
"#;
        let program = compile_source(input).unwrap();
        let deploy = generate_deploy(&program);

        assert_eq!(deploy.infrastructure.streams.len(), 1);
        assert_eq!(deploy.infrastructure.streams[0].transport, "websocket");
    }

    #[test]
    fn test_deploy_full_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        )
        .unwrap();
        let program = compile_source(&input).unwrap();
        let deploy = generate_deploy(&program);

        assert!(!deploy.infrastructure.databases.is_empty());
        assert!(deploy.compose.contains("services:"));
        assert!(deploy.kubernetes.contains("axis-app"));
    }

    #[test]
    fn test_deploy_json_serializes() {
        let input = r#"SHAPE User
  id UUID PK AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#;
        let program = compile_source(input).unwrap();
        let deploy = generate_deploy(&program);
        let json = serde_json::to_string_pretty(&deploy.infrastructure).unwrap();
        assert!(json.contains("postgres"));
        assert!(json.contains("8080"));
    }
}
