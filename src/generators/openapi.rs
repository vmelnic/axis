use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::ast::*;

pub fn generate_openapi(program: &Program) -> Value {
    let mut builder = OpenApiGen::new(program);
    builder.generate()
}

struct OpenApiGen<'a> {
    shapes: BTreeMap<String, &'a ShapeDef>,
    sources: BTreeMap<String, &'a SourceDef>,
    flows: Vec<&'a FlowDef>,
    surfaces: Vec<&'a SurfaceDef>,
    streams: Vec<&'a StreamDef>,
}

impl<'a> OpenApiGen<'a> {
    fn new(program: &'a Program) -> Self {
        let mut shapes = BTreeMap::new();
        let mut sources = BTreeMap::new();
        let mut flows = Vec::new();
        let mut surfaces = Vec::new();
        let mut streams = Vec::new();

        for c in &program.constructs {
            match c {
                Construct::Shape(s) => { shapes.insert(s.name.clone(), s); }
                Construct::Source(s) => { sources.insert(s.name.clone(), s); }
                Construct::Flow(f) => flows.push(f),
                Construct::Surface(s) => surfaces.push(s),
                Construct::Stream(s) => streams.push(s),
                _ => {}
            }
        }

        Self { shapes, sources, flows, surfaces, streams }
    }

    fn generate(&mut self) -> Value {
        let (title, version, base_path) = if let Some(surface) = self.surfaces.first() {
            (
                surface.name.clone(),
                surface.version.clone(),
                surface.base_path.clone().unwrap_or_default(),
            )
        } else {
            ("API".into(), "1.0".into(), String::new())
        };

        let mut paths = Map::new();
        let mut used_schemas: BTreeMap<String, Value> = BTreeMap::new();
        let mut security_schemes: BTreeMap<String, Value> = BTreeMap::new();

        if let Some(surface) = self.surfaces.first() {
            for route in &surface.routes {
                let flow = self.flows.iter().find(|f| f.name == route.target);
                let oapi_path = convert_path_params(&format!("{}{}", base_path, route.path));
                let method_str = http_method_str(&route.method).to_lowercase();

                let operation = if let Some(flow) = flow {
                    self.build_operation(flow, &mut used_schemas, &mut security_schemes)
                } else {
                    json!({ "operationId": route.target })
                };

                let path_entry = paths.entry(oapi_path).or_insert_with(|| json!({}));
                path_entry[method_str] = operation;
            }

            for expose in &surface.exposes {
                let schema = self.build_expose_schema(expose);
                let name = expose.alias.as_ref().unwrap_or(&expose.shape).clone();
                used_schemas.insert(name, schema);
            }
        } else {
            for flow in &self.flows {
                let oapi_path = convert_path_params(&flow.path);
                let method_str = http_method_str(&flow.method).to_lowercase();
                let operation = self.build_operation(flow, &mut used_schemas, &mut security_schemes);
                let path_entry = paths.entry(oapi_path).or_insert_with(|| json!({}));
                path_entry[method_str] = operation;
            }
        }

        for stream in &self.streams {
            let oapi_path = convert_path_params(&stream.path);
            let transport = match stream.transport {
                StreamTransport::WebSocket => "websocket",
                StreamTransport::Sse => "text/event-stream",
            };
            let event_schemas: Vec<Value> = stream.events.iter().map(|evt| {
                let mut props = Map::new();
                for f in &evt.fields {
                    props.insert(f.name.clone(), self.type_to_schema(&f.ty));
                }
                json!({
                    "type": "object",
                    "properties": Value::Object(props),
                    "x-event-name": evt.name
                })
            }).collect();

            let operation = json!({
                "operationId": format!("stream_{}", stream.name),
                "x-transport": transport,
                "responses": {
                    "101": {
                        "description": format!("{} stream upgrade", transport)
                    }
                },
                "x-events": event_schemas
            });
            let path_entry = paths.entry(oapi_path).or_insert_with(|| json!({}));
            path_entry["get"] = operation;
        }

        let schemas_value: Map<String, Value> = used_schemas.into_iter().collect();
        let sec_schemes_value: Map<String, Value> = security_schemes.into_iter().collect();

        let mut spec = json!({
            "openapi": "3.1.0",
            "info": {
                "title": title,
                "version": version
            },
            "paths": Value::Object(paths)
        });

        let mut components = json!({});
        if !schemas_value.is_empty() {
            components["schemas"] = Value::Object(schemas_value);
        }
        if !sec_schemes_value.is_empty() {
            components["securitySchemes"] = Value::Object(sec_schemes_value);
        }
        if components.as_object().is_some_and(|o| !o.is_empty()) {
            spec["components"] = components;
        }

        if let Some(surface) = self.surfaces.first() {
            if let Some(dep) = &surface.deprecate {
                spec["x-deprecation"] = json!({
                    "replaces": dep.version,
                    "sunset": dep.sunset
                });
            }
        }

        spec
    }

    fn build_operation(
        &self,
        flow: &FlowDef,
        schemas: &mut BTreeMap<String, Value>,
        security_schemes: &mut BTreeMap<String, Value>,
    ) -> Value {
        let mut op = json!({
            "operationId": flow.name,
        });

        if let Some(auth) = &flow.auth {
            let (scheme_name, scheme_def, requirement) = self.auth_to_security(auth);
            security_schemes.insert(scheme_name.clone(), scheme_def);
            op["security"] = json!([{ scheme_name: requirement }]);
        }

        let mut parameters = Vec::new();

        let path_params = extract_path_params(&flow.path);
        for param in &path_params {
            parameters.push(json!({
                "name": param,
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            }));
        }

        for param in &flow.params {
            let schema = self.type_to_schema(&param.ty);
            let required = param.modifiers.iter().any(|m| matches!(m, Modifier::Required));
            let mut p = json!({
                "name": param.name,
                "in": "query",
                "schema": schema
            });
            if required {
                p["required"] = json!(true);
            }
            for m in &param.modifiers {
                if let Modifier::Default(v) = m {
                    p["schema"]["default"] = literal_to_json(v);
                }
            }
            parameters.push(p);
        }

        for header in &flow.headers {
            let schema = self.type_to_schema(&header.ty);
            let required = header.modifiers.iter().any(|m| matches!(m, Modifier::Required));
            let mut h = json!({
                "name": header.name,
                "in": "header",
                "schema": schema
            });
            if required {
                h["required"] = json!(true);
            }
            parameters.push(h);
        }

        if !parameters.is_empty() {
            op["parameters"] = Value::Array(parameters);
        }

        if let Some(body) = &flow.body {
            let body_schema = self.build_body_schema(body);
            let schema_name = &body.name;
            schemas.insert(schema_name.clone(), body_schema);
            op["requestBody"] = json!({
                "required": true,
                "content": {
                    "application/json": {
                        "schema": {
                            "$ref": format!("#/components/schemas/{schema_name}")
                        }
                    }
                }
            });
        }

        let mut responses = Map::new();

        let success_code = flow.return_stmt.code.to_string();
        let mut success_response = json!({ "description": "Success" });

        if let Some(body) = &flow.return_stmt.body {
            match body {
                ReturnBody::Binding(name) => {
                    let return_schema = self.infer_binding_schema(flow, name);
                    success_response["content"] = json!({
                        "application/json": {
                            "schema": return_schema
                        }
                    });
                }
                ReturnBody::Inline(fields) => {
                    let schema = self.build_inline_return_schema(fields);
                    success_response["content"] = json!({
                        "application/json": {
                            "schema": schema
                        }
                    });
                }
                ReturnBody::Paginated { .. } => {
                    success_response["content"] = json!({
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "items": { "type": "array" },
                                    "total": { "type": "integer" },
                                    "next_cursor": { "type": "string" },
                                    "has_more": { "type": "boolean" }
                                }
                            }
                        }
                    });
                }
            }
        }
        responses.insert(success_code, success_response);

        let mut error_codes: BTreeMap<String, String> = BTreeMap::new();
        for step in &flow.steps {
            match step {
                FlowStep::Guard(g) => {
                    let code = g.code.to_string();
                    let msg = g.message.as_deref().unwrap_or("Guard failed");
                    error_codes.entry(code).or_insert_with(|| msg.to_string());
                }
                FlowStep::Let(l) => {
                    if let Expr::Fetch { or_code, or_message, .. } = &l.expr {
                        if *or_code > 0 {
                            let code = or_code.to_string();
                            let msg = or_message.as_deref().unwrap_or("Not found");
                            error_codes.entry(code).or_insert_with(|| msg.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        for (code, msg) in &error_codes {
            responses.insert(code.clone(), json!({
                "description": msg,
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "properties": {
                                "error": { "type": "string" },
                                "code": { "type": "integer" }
                            }
                        }
                    }
                }
            }));
        }

        if !flow.limits.is_empty() {
            responses.insert("429".into(), json!({
                "description": "Rate limit exceeded",
                "headers": {
                    "Retry-After": {
                        "schema": { "type": "integer" },
                        "description": "Seconds until rate limit resets"
                    }
                }
            }));
        }

        op["responses"] = Value::Object(responses);

        if !flow.limits.is_empty() {
            let limit = &flow.limits[0];
            op["x-rate-limit"] = json!({
                "limit": limit.count,
                "window": rate_unit_str(&limit.unit),
                "scope": rate_scope_str(&limit.scope)
            });
        }

        if !flow.cache.is_empty() {
            let cache = &flow.cache[0];
            op["x-cache"] = json!({
                "ttl": cache.ttl,
                "vary": cache.vary.iter().map(|v| v.as_str()).collect::<Vec<_>>()
            });
        }

        op
    }

    fn auth_to_security(&self, auth: &AuthDecl) -> (String, Value, Value) {
        match auth {
            AuthDecl::Session => (
                "session".into(),
                json!({ "type": "apiKey", "in": "cookie", "name": "session_id" }),
                json!([]),
            ),
            AuthDecl::Bearer => (
                "bearer".into(),
                json!({ "type": "http", "scheme": "bearer" }),
                json!([]),
            ),
            AuthDecl::ApiKey => (
                "api_key".into(),
                json!({ "type": "apiKey", "in": "header", "name": "X-API-Key" }),
                json!([]),
            ),
            AuthDecl::Role(role) => (
                "bearer".into(),
                json!({ "type": "http", "scheme": "bearer" }),
                json!([role]),
            ),
            AuthDecl::RoleIn(roles) => (
                "bearer".into(),
                json!({ "type": "http", "scheme": "bearer" }),
                json!(roles),
            ),
            AuthDecl::WebhookSignature { algorithm, .. } => (
                "webhook_signature".into(),
                json!({
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Webhook-Signature",
                    "x-algorithm": algorithm
                }),
                json!([]),
            ),
            AuthDecl::None => (
                "none".into(),
                json!({ "type": "http", "scheme": "none" }),
                json!([]),
            ),
        }
    }

    fn build_body_schema(&self, body: &BodyDecl) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for field in &body.fields {
            let schema = self.type_to_schema(&field.ty);
            properties.insert(field.name.clone(), schema);
            if field.modifiers.iter().any(|m| matches!(m, Modifier::Required)) {
                required.push(json!(field.name));
            }
        }

        let mut schema = json!({
            "type": "object",
            "properties": Value::Object(properties)
        });
        if !required.is_empty() {
            schema["required"] = Value::Array(required);
        }
        schema
    }

    fn build_expose_schema(&self, expose: &ExposeDef) -> Value {
        let mut properties = Map::new();
        let mut hidden = Vec::new();
        let mut renames: BTreeMap<String, String> = BTreeMap::new();

        for ef in &expose.fields {
            match ef {
                ExposeField::Field { name, ty } => {
                    properties.insert(name.clone(), self.type_to_schema(ty));
                }
                ExposeField::Hide(name) => {
                    hidden.push(name.clone());
                }
                ExposeField::Rename { from, to } => {
                    renames.insert(from.clone(), to.clone());
                }
            }
        }

        let mut final_props = Map::new();
        for (name, schema) in &properties {
            if hidden.contains(name) {
                continue;
            }
            let display_name = renames.get(name).unwrap_or(name);
            final_props.insert(display_name.clone(), schema.clone());
        }

        json!({
            "type": "object",
            "properties": Value::Object(final_props)
        })
    }

    fn build_inline_return_schema(&self, fields: &[ReturnField]) -> Value {
        let mut properties = Map::new();
        for field in fields {
            match &field.value {
                ReturnValue::Expr(_) => {
                    properties.insert(field.name.clone(), json!({ "type": "string" }));
                }
                ReturnValue::Nested(sub) => {
                    properties.insert(field.name.clone(), self.build_inline_return_schema(sub));
                }
            }
        }
        json!({
            "type": "object",
            "properties": Value::Object(properties)
        })
    }

    fn infer_binding_schema(&self, flow: &FlowDef, binding: &str) -> Value {
        for step in &flow.steps {
            match step {
                FlowStep::Let(l) if l.name == binding => {
                    return match &l.expr {
                        Expr::Fetch { source, .. } => {
                            self.source_shape_schema(source)
                        }
                        Expr::Query { source, .. } => {
                            json!({
                                "type": "array",
                                "items": self.source_shape_schema(source)
                            })
                        }
                        _ => json!({ "type": "object" }),
                    };
                }
                FlowStep::Insert(i) if i.binding.as_deref() == Some(binding) => {
                    return self.source_shape_schema(&i.source);
                }
                FlowStep::Update(u) => {
                    let update_binding = u.binding.as_ref().map(|b| match b {
                        UpdateBinding::As(name) | UpdateBinding::Count(name) => name.as_str(),
                    });
                    if update_binding == Some(binding) {
                        return self.source_shape_schema(&u.source);
                    }
                }
                _ => {}
            }
        }
        json!({ "type": "object" })
    }

    fn source_shape_schema(&self, source_name: &str) -> Value {
        let shape_name = self.sources.get(source_name)
            .map(|s| s.shape.as_str());

        if let Some(shape_name) = shape_name {
            if let Some(shape) = self.shapes.get(shape_name) {
                return self.shape_to_schema(shape);
            }
        }
        json!({ "type": "object" })
    }

    fn shape_to_schema(&self, shape: &ShapeDef) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for field in &shape.fields {
            let is_maybe = matches!(&field.ty, TypeExpr::Maybe(_));
            let schema = self.type_to_schema(&field.ty);
            properties.insert(field.name.clone(), schema);
            let is_required = field.modifiers.iter().any(|m| matches!(m, Modifier::Required));
            let is_pk = field.modifiers.iter().any(|m| matches!(m, Modifier::Pk));
            if (is_required || is_pk) && !is_maybe {
                required.push(json!(field.name));
            }
        }

        let mut schema = json!({
            "type": "object",
            "properties": Value::Object(properties)
        });
        if !required.is_empty() {
            schema["required"] = Value::Array(required);
        }
        schema
    }

    fn type_to_schema(&self, ty: &TypeExpr) -> Value {
        match ty {
            TypeExpr::Uuid => json!({ "type": "string", "format": "uuid" }),
            TypeExpr::Bool => json!({ "type": "boolean" }),
            TypeExpr::Date => json!({ "type": "string", "format": "date" }),
            TypeExpr::Timestamp => json!({ "type": "string", "format": "date-time" }),
            TypeExpr::Text => json!({ "type": "string" }),
            TypeExpr::String(Some(n)) => json!({ "type": "string", "maxLength": n }),
            TypeExpr::String(None) => json!({ "type": "string" }),
            TypeExpr::Int { min, max } => {
                let mut s = json!({ "type": "integer" });
                if let Some(v) = min { s["minimum"] = json!(v); }
                if let Some(v) = max { s["maximum"] = json!(v); }
                s
            }
            TypeExpr::Decimal { .. } => json!({ "type": "number" }),
            TypeExpr::Enum(variants) => json!({ "type": "string", "enum": variants }),
            TypeExpr::Json => json!({ "type": "object" }),
            TypeExpr::List(inner) => json!({
                "type": "array",
                "items": self.type_to_schema(inner)
            }),
            TypeExpr::Map(_, v) => json!({
                "type": "object",
                "additionalProperties": self.type_to_schema(v)
            }),
            TypeExpr::Ref { .. } => json!({ "type": "string", "format": "uuid" }),
            TypeExpr::Blob => json!({ "type": "string", "format": "binary" }),
            TypeExpr::Maybe(inner) => {
                let mut s = self.type_to_schema(inner);
                s["nullable"] = json!(true);
                s
            }
        }
    }
}

fn convert_path_params(path: &str) -> String {
    let mut result = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            result.push('{');
            while let Some(&nc) = chars.peek() {
                if nc == '/' {
                    break;
                }
                result.push(nc);
                chars.next();
            }
            result.push('}');
        } else {
            result.push(c);
        }
    }
    result
}

fn extract_path_params(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| s.starts_with(':'))
        .map(|s| s[1..].to_string())
        .collect()
}

fn http_method_str(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Webhook => "POST",
    }
}

fn rate_unit_str(unit: &RateUnit) -> &'static str {
    match unit {
        RateUnit::PerSecond => "second",
        RateUnit::PerMinute => "minute",
        RateUnit::PerHour => "hour",
        RateUnit::PerDay => "day",
    }
}

fn rate_scope_str(scope: &RateScope) -> &'static str {
    match scope {
        RateScope::PerUser => "user",
        RateScope::PerIp => "ip",
        RateScope::PerKey => "key",
        RateScope::Global => "global",
    }
}

fn literal_to_json(val: &LiteralValue) -> Value {
    match val {
        LiteralValue::Int(n) => json!(n),
        LiteralValue::Decimal(d) => {
            if let Ok(f) = d.parse::<f64>() { json!(f) } else { json!(d) }
        }
        LiteralValue::String(s) => json!(s),
        LiteralValue::Bool(b) => json!(b),
        LiteralValue::Ident(s) => json!(s),
        LiteralValue::Now => json!("NOW"),
        LiteralValue::None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn gen_openapi(input: &str) -> Value {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        generate_openapi(&program)
    }

    #[test]
    fn test_basic_openapi() {
        let spec = gen_openapi(r#"SHAPE User
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
"#);
        assert_eq!(spec["openapi"], "3.1.0");
        assert_eq!(spec["info"]["title"], "API");
        let paths = spec["paths"].as_object().unwrap();
        assert!(paths.contains_key("/users/{id}"));
        let get = &paths["/users/{id}"]["get"];
        assert_eq!(get["operationId"], "get_user");
        assert!(get["security"].is_array());
        assert!(get["responses"]["200"].is_object());
        assert!(get["responses"]["404"].is_object());
    }

    #[test]
    fn test_openapi_with_surface() {
        let spec = gen_openapi(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED

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

SURFACE public v2
  REALM api
  BASE_PATH /api/v2
  ROUTE GET /users/:id -> get_user
  EXPOSE User AS UserResponse
    FIELD id UUID
    FIELD name STRING
    HIDE email
"#);
        assert_eq!(spec["info"]["title"], "public");
        assert_eq!(spec["info"]["version"], "v2");
        let paths = spec["paths"].as_object().unwrap();
        assert!(paths.contains_key("/api/v2/users/{id}"));
        let schemas = &spec["components"]["schemas"];
        assert!(schemas["UserResponse"].is_object());
        let props = schemas["UserResponse"]["properties"].as_object().unwrap();
        assert!(props.contains_key("id"));
        assert!(props.contains_key("name"));
        assert!(!props.contains_key("email"));
    }

    #[test]
    fn test_openapi_request_body() {
        let spec = gen_openapi(r#"SHAPE Order
  id UUID PK AUTO
  user_id UUID REQUIRED
  quantity INT MIN 1 MAX 100 REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders
  CAPABILITY write orders

FLOW create_order post /orders
  REALM api
  AUTH session
  LIMIT 20 PER_MINUTE PER_USER
  BODY OrderCreate
    user_id UUID REQUIRED
    quantity INT MIN 1 MAX 100 REQUIRED
  INSERT orders
    user_id body.user_id
    quantity body.quantity
  AS order
  RETURN 201 order
"#);
        let post = &spec["paths"]["/orders"]["post"];
        assert!(post["requestBody"].is_object());
        let ref_path = post["requestBody"]["content"]["application/json"]["schema"]["$ref"].as_str().unwrap();
        assert_eq!(ref_path, "#/components/schemas/OrderCreate");

        let schema = &spec["components"]["schemas"]["OrderCreate"];
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("user_id"));
        assert!(props.contains_key("quantity"));
        assert_eq!(props["quantity"]["minimum"], 1);
        assert_eq!(props["quantity"]["maximum"], 100);
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("user_id")));
        assert!(required.contains(&json!("quantity")));

        assert!(post["responses"]["429"].is_object());
        assert_eq!(post["x-rate-limit"]["limit"], 20);
    }

    #[test]
    fn test_openapi_query_params() {
        let spec = gen_openapi(r#"SHAPE Order
  id UUID PK AUTO
  status ENUM pending confirmed REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders

FLOW list_orders get /orders
  REALM api
  AUTH session
  PARAM page_size INT DEFAULT 20 MIN 1 MAX 100
  PARAM status MAYBE ENUM pending confirmed
  LET orders
    QUERY orders
      FILTER id EQ auth.user_id
  RETURN 200 orders
"#);
        let get = &spec["paths"]["/orders"]["get"];
        let params = get["parameters"].as_array().unwrap();
        let page_size = params.iter().find(|p| p["name"] == "page_size").unwrap();
        assert_eq!(page_size["in"], "query");
        assert_eq!(page_size["schema"]["type"], "integer");
        assert_eq!(page_size["schema"]["default"], 20);
    }

    #[test]
    fn test_openapi_guard_errors() {
        let spec = gen_openapi(r#"SHAPE Booking
  id UUID PK AUTO
  user_id UUID REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

REALM api
  CAPABILITY read bookings

FLOW get_booking get /bookings/:id
  REALM api
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  GUARD ownership 403 "not your booking"
    EQ booking.user_id auth.user_id
  RETURN 200 booking
"#);
        let get = &spec["paths"]["/bookings/{id}"]["get"];
        let responses = get["responses"].as_object().unwrap();
        assert!(responses.contains_key("200"));
        assert!(responses.contains_key("403"));
        assert!(responses.contains_key("404"));
        assert_eq!(responses["403"]["description"], "not your booking");
    }

    #[test]
    fn test_openapi_response_schema() {
        let spec = gen_openapi(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
  email STRING 255 REQUIRED UNIQUE

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
"#);
        let get = &spec["paths"]["/users/{id}"]["get"];
        let schema = &get["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("id"));
        assert!(props.contains_key("name"));
        assert!(props.contains_key("email"));
        assert_eq!(props["id"]["format"], "uuid");
        assert_eq!(props["email"]["maxLength"], 255);
    }

    #[test]
    fn test_openapi_list_response() {
        let spec = gen_openapi(r#"SHAPE Order
  id UUID PK AUTO
  status ENUM pending confirmed REQUIRED

SOURCE orders POSTGRES
  SHAPE Order
  INDEX id

REALM api
  CAPABILITY read orders

FLOW list_orders get /orders
  REALM api
  AUTH session
  LET orders
    QUERY orders
      FILTER id EQ auth.user_id
  RETURN 200 orders
"#);
        let get = &spec["paths"]["/orders"]["get"];
        let schema = &get["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(schema["type"], "array");
        assert!(schema["items"]["properties"].is_object());
    }

    #[test]
    fn test_openapi_cache_extension() {
        let spec = gen_openapi(r#"SHAPE User
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
  CACHE 60 VARY path.id auth.user_id
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#);
        let get = &spec["paths"]["/users/{id}"]["get"];
        assert_eq!(get["x-cache"]["ttl"], 60);
        let vary = get["x-cache"]["vary"].as_array().unwrap();
        assert_eq!(vary.len(), 2);
    }

    #[test]
    fn test_openapi_booking_full() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis")
        ).unwrap();
        let spec = gen_openapi(&input);
        assert_eq!(spec["openapi"], "3.1.0");
        let paths = spec["paths"].as_object().unwrap();
        assert_eq!(paths.len(), 2); // /bookings/:id and /bookings
        let json = serde_json::to_string_pretty(&spec).unwrap();
        assert!(json.contains("\"operationId\""));
        assert!(json.contains("session"));
    }

    #[test]
    fn test_openapi_full_example() {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis")
        ).unwrap();
        let spec = gen_openapi(&input);
        let paths = spec["paths"].as_object().unwrap();
        assert_eq!(paths.len(), 3); // /api/v1/orders, /api/v1/orders/:id, /ws/orders
        assert!(spec["components"]["schemas"]["OrderResponse"].is_object());

        let order_schema = &spec["components"]["schemas"]["OrderResponse"];
        let props = order_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("id"));
        assert!(props.contains_key("status"));
        assert!(!props.contains_key("user_id"));
        assert!(!props.contains_key("item_id"));
    }

    #[test]
    fn test_openapi_deprecation() {
        let spec = gen_openapi(r#"SHAPE Item
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW list_items get /items
  AUTH session
  LET items
    QUERY items
      FILTER id EQ auth.user_id
  RETURN 200 items

SURFACE shop v2
  BASE_PATH /api/v2
  ROUTE GET /items -> list_items
  EXPOSE Item AS ItemResponse
    FIELD id UUID
    FIELD name STRING
  DEPRECATE v1 SUNSET "2027-06-01"
"#);
        assert_eq!(spec["x-deprecation"]["replaces"], "v1");
        assert_eq!(spec["x-deprecation"]["sunset"], "2027-06-01");
    }

    #[test]
    fn test_openapi_rename_fields() {
        let spec = gen_openapi(r#"SHAPE Booking
  id UUID PK AUTO
  note STRING 500 REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX id

FLOW get_booking get /bookings/:id
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  RETURN 200 booking

SURFACE public v1
  ROUTE GET /bookings/:id -> get_booking
  EXPOSE Booking AS BookingResponse
    FIELD id UUID
    FIELD note STRING
    RENAME note AS special_requests
"#);
        let props = spec["components"]["schemas"]["BookingResponse"]["properties"].as_object().unwrap();
        assert!(props.contains_key("special_requests"));
        assert!(!props.contains_key("note"));
    }

    #[test]
    fn test_openapi_maybe_nullable() {
        let spec = gen_openapi(r#"SHAPE Item
  id UUID PK AUTO
  name STRING 100 REQUIRED
  description MAYBE TEXT

SOURCE items POSTGRES
  SHAPE Item
  INDEX id

FLOW get_item get /items/:id
  AUTH session
  LET item
    FETCH items
      FILTER id EQ path.id
    OR 404
  RETURN 200 item
"#);
        let schema = &spec["paths"]["/items/{id}"]["get"]["responses"]["200"]["content"]["application/json"]["schema"];
        let desc = &schema["properties"]["description"];
        assert_eq!(desc["nullable"], true);
    }

    #[test]
    fn test_openapi_stream() {
        let spec = gen_openapi(r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

STREAM notifications ws /ws/notifications
  AUTH session
  EVENT user_joined
    user_id UUID
    name STRING 100
"#);
        let paths = spec["paths"].as_object().unwrap();
        assert!(paths.contains_key("/ws/notifications"));
        let get = &paths["/ws/notifications"]["get"];
        assert_eq!(get["operationId"], "stream_notifications");
        assert_eq!(get["x-transport"], "websocket");
        let events = get["x-events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["x-event-name"], "user_joined");
    }
}
