use crate::ast::*;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TestSuite {
    pub tests: Vec<GeneratedTest>,
}

#[derive(Debug, Serialize)]
pub struct GeneratedTest {
    pub name: String,
    pub flow: String,
    pub category: TestCategory,
    pub request: TestRequest,
    pub expected: TestExpectation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCategory {
    TypeInvariant,
    AuthInvariant,
    TenantIsolation,
    GuardInvariant,
    Idempotency,
    StreamConnect,
}

#[derive(Debug, Serialize)]
pub struct TestRequest {
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct TestExpectation {
    pub status: ExpectedStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_matches_shape: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedStatus {
    Exactly(i64),
    OneOf(Vec<i64>),
    Not(Vec<i64>),
}

pub fn generate_tests(program: &Program) -> TestSuite {
    let mut tests = Vec::new();

    for construct in &program.constructs {
        match construct {
            Construct::Flow(flow) => {
                tests.extend(generate_flow_tests(flow, program));
            }
            Construct::Stream(stream) => {
                tests.push(generate_stream_connect_test(stream));
            }
            _ => {}
        }
    }

    TestSuite { tests }
}

fn generate_flow_tests(flow: &FlowDef, program: &Program) -> Vec<GeneratedTest> {
    let mut tests = Vec::new();
    let method_str = format!("{:?}", flow.method).to_uppercase();

    tests.push(generate_type_invariant(flow, &method_str, program));

    if flow.auth.is_some() {
        tests.push(generate_auth_invariant_no_auth(flow, &method_str));
    }

    if flow.scope.is_some() {
        tests.push(generate_tenant_isolation(flow, &method_str));
    }

    for step in &flow.steps {
        if let FlowStep::Guard(guard) = step {
            tests.push(generate_guard_invariant(flow, guard, &method_str));
        }
    }

    let has_idempotency_header = flow.headers.iter().any(|h| h.name == "idempotency_key");
    if has_idempotency_header && matches!(flow.method, HttpMethod::Post) {
        tests.push(generate_idempotency_test(flow, &method_str));
    }

    tests
}

fn generate_type_invariant(flow: &FlowDef, method: &str, program: &Program) -> GeneratedTest {
    let body_fields = flow.body.as_ref().map(|b| &b.fields);
    let body = body_fields.map(|f| build_valid_body(f));
    let return_shape = flow.return_stmt.body.as_ref().and_then(|b| {
        match b {
            ReturnBody::Binding(name) => find_binding_shape(flow, name, program),
            ReturnBody::Inline(_) | ReturnBody::Paginated { .. } => None,
        }
    });

    GeneratedTest {
        name: format!("{}_type_invariant", flow.name),
        flow: flow.name.clone(),
        category: TestCategory::TypeInvariant,
        request: TestRequest {
            method: method.to_string(),
            path: flow.path.clone(),
            body,
            auth: flow.auth.as_ref().map(|_| "valid_token".into()),
            headers: vec![],
        },
        expected: TestExpectation {
            status: ExpectedStatus::Exactly(flow.return_stmt.code),
            body_matches_shape: return_shape,
        },
    }
}

fn generate_auth_invariant_no_auth(flow: &FlowDef, method: &str) -> GeneratedTest {
    GeneratedTest {
        name: format!("{}_auth_required", flow.name),
        flow: flow.name.clone(),
        category: TestCategory::AuthInvariant,
        request: TestRequest {
            method: method.to_string(),
            path: flow.path.clone(),
            body: None,
            auth: None,
            headers: vec![],
        },
        expected: TestExpectation {
            status: ExpectedStatus::OneOf(vec![401, 403]),
            body_matches_shape: None,
        },
    }
}

fn generate_tenant_isolation(flow: &FlowDef, method: &str) -> GeneratedTest {
    GeneratedTest {
        name: format!("{}_tenant_isolation", flow.name),
        flow: flow.name.clone(),
        category: TestCategory::TenantIsolation,
        request: TestRequest {
            method: method.to_string(),
            path: flow.path.clone(),
            body: None,
            auth: Some("user_a_token".into()),
            headers: vec![],
        },
        expected: TestExpectation {
            status: ExpectedStatus::Not(vec![500]),
            body_matches_shape: None,
        },
    }
}

fn generate_guard_invariant(flow: &FlowDef, guard: &GuardStep, method: &str) -> GeneratedTest {
    GeneratedTest {
        name: format!("{}_{}_guard_blocks", flow.name, guard.name),
        flow: flow.name.clone(),
        category: TestCategory::GuardInvariant,
        request: TestRequest {
            method: method.to_string(),
            path: flow.path.clone(),
            body: None,
            auth: flow.auth.as_ref().map(|_| "valid_token".into()),
            headers: vec![],
        },
        expected: TestExpectation {
            status: ExpectedStatus::Exactly(guard.code),
            body_matches_shape: None,
        },
    }
}

fn generate_idempotency_test(flow: &FlowDef, method: &str) -> GeneratedTest {
    let body = flow.body.as_ref().map(|b| build_valid_body(&b.fields));
    GeneratedTest {
        name: format!("{}_idempotent", flow.name),
        flow: flow.name.clone(),
        category: TestCategory::Idempotency,
        request: TestRequest {
            method: method.to_string(),
            path: flow.path.clone(),
            body,
            auth: flow.auth.as_ref().map(|_| "valid_token".into()),
            headers: vec![("Idempotency-Key".into(), "test-key-123".into())],
        },
        expected: TestExpectation {
            status: ExpectedStatus::Exactly(flow.return_stmt.code),
            body_matches_shape: None,
        },
    }
}

fn build_valid_body(fields: &[FieldDef]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for field in fields {
        let val = sample_value_for_type(&field.ty);
        map.insert(field.name.clone(), val);
    }
    serde_json::Value::Object(map)
}

fn sample_value_for_type(ty: &TypeExpr) -> serde_json::Value {
    match ty {
        TypeExpr::Uuid => serde_json::json!("00000000-0000-0000-0000-000000000001"),
        TypeExpr::String(_) => serde_json::json!("test_value"),
        TypeExpr::Text => serde_json::json!("test text content"),
        TypeExpr::Int { .. } => serde_json::json!(1),
        TypeExpr::Decimal { .. } => serde_json::json!("10.00"),
        TypeExpr::Bool => serde_json::json!(true),
        TypeExpr::Date => serde_json::json!("2025-01-01"),
        TypeExpr::Timestamp => serde_json::json!("2025-01-01T00:00:00Z"),
        TypeExpr::Enum(variants) => {
            serde_json::json!(variants.first().cloned().unwrap_or_default())
        }
        TypeExpr::Json => serde_json::json!({}),
        TypeExpr::Ref { shape, .. } => serde_json::json!(format!("ref_{}", shape)),
        TypeExpr::List(inner) => serde_json::json!([sample_value_for_type(inner)]),
        TypeExpr::Map(k, v) => {
            let mut map = serde_json::Map::new();
            let key = match sample_value_for_type(k) {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            map.insert(key, sample_value_for_type(v));
            serde_json::Value::Object(map)
        }
        TypeExpr::Blob => serde_json::json!("<binary>"),
        TypeExpr::Maybe(inner) => sample_value_for_type(inner),
    }
}

fn generate_stream_connect_test(stream: &StreamDef) -> GeneratedTest {
    let transport = match stream.transport {
        StreamTransport::WebSocket => "websocket",
        StreamTransport::Sse => "sse",
    };
    GeneratedTest {
        name: format!("{}_stream_connect", stream.name),
        flow: stream.name.clone(),
        category: TestCategory::StreamConnect,
        request: TestRequest {
            method: "GET".to_string(),
            path: stream.path.clone(),
            body: None,
            auth: stream.auth.as_ref().map(|_| "valid_token".into()),
            headers: vec![("Upgrade".into(), transport.into())],
        },
        expected: TestExpectation {
            status: ExpectedStatus::OneOf(vec![101, 200]),
            body_matches_shape: None,
        },
    }
}

fn find_binding_shape(_flow: &FlowDef, _name: &str, _program: &Program) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_program() -> Program {
        let input = r#"SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED
  role ENUM admin user REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id
  INDEX email UNIQUE

REALM api
  TENANT user_id
  CAPABILITY read users
  CAPABILITY write users

FLOW get_user get /users/:id
  REALM api
  AUTH session
  SCOPE TENANT user_id
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  GUARD ownership 403 "not yours"
    EQ user.id auth.user_id
  RETURN 200 user

FLOW create_user post /users
  REALM api
  AUTH role admin
  BODY UserCreate
    email STRING 255 REQUIRED
    name STRING 100 REQUIRED
  INSERT users
    email body.email
    name body.name
  AS user
  RETURN 201 user
"#;
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = crate::parser::Parser::new(tokens);
        parser.parse_program().unwrap()
    }

    #[test]
    fn test_generate_tests_produces_cases() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        assert!(suite.tests.len() >= 4);
    }

    #[test]
    fn test_type_invariant_generated() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        let ti = suite.tests.iter().find(|t| t.name == "get_user_type_invariant").unwrap();
        assert!(matches!(ti.expected.status, ExpectedStatus::Exactly(200)));
        assert_eq!(ti.request.method, "GET");
    }

    #[test]
    fn test_auth_invariant_generated() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        let ai = suite.tests.iter().find(|t| t.name == "get_user_auth_required").unwrap();
        assert!(matches!(ai.expected.status, ExpectedStatus::OneOf(ref codes) if codes.contains(&401)));
        assert!(ai.request.auth.is_none());
    }

    #[test]
    fn test_tenant_isolation_generated() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        let ti = suite.tests.iter().find(|t| t.name == "get_user_tenant_isolation").unwrap();
        assert!(matches!(ti.category, TestCategory::TenantIsolation));
    }

    #[test]
    fn test_guard_invariant_generated() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        let gi = suite.tests.iter().find(|t| t.name == "get_user_ownership_guard_blocks").unwrap();
        assert!(matches!(gi.expected.status, ExpectedStatus::Exactly(403)));
    }

    #[test]
    fn test_body_generation() {
        let fields = vec![
            FieldDef {
                name: "email".into(),
                ty: TypeExpr::String(Some(255)),
                modifiers: vec![],
                span: crate::token::Span { offset: 0, len: 0, line: 1, col: 1 },
            },
            FieldDef {
                name: "count".into(),
                ty: TypeExpr::Int { min: None, max: None },
                modifiers: vec![],
                span: crate::token::Span { offset: 0, len: 0, line: 1, col: 1 },
            },
        ];
        let body = build_valid_body(&fields);
        assert!(body.get("email").unwrap().is_string());
        assert!(body.get("count").unwrap().is_number());
    }

    #[test]
    fn test_serializes_to_json() {
        let program = make_test_program();
        let suite = generate_tests(&program);
        let json = serde_json::to_string_pretty(&suite).unwrap();
        assert!(json.contains("type_invariant"));
        assert!(json.contains("auth_invariant"));
    }

    #[test]
    fn test_stream_connect_test_generated() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

STREAM notifications ws /ws/notifications
  AUTH session
  EVENT user_online
    user_id UUID
"#;
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        let suite = generate_tests(&program);
        let st = suite.tests.iter().find(|t| t.name == "notifications_stream_connect").unwrap();
        assert!(matches!(st.category, TestCategory::StreamConnect));
        assert_eq!(st.request.method, "GET");
        assert_eq!(st.request.path, "/ws/notifications");
        assert!(st.request.headers.iter().any(|(k, _)| k == "Upgrade"));
    }
}
