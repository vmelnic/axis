use std::fmt::Write;

use crate::ast::*;

#[derive(Debug)]
pub struct WasmProject {
    pub cargo_toml: String,
    pub modules: Vec<WasmModule>,
}

#[derive(Debug)]
pub struct WasmModule {
    pub name: String,
    pub source: String,
}

pub fn generate(program: &Program) -> WasmProject {
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

    let modules = flows
        .iter()
        .map(|flow| WasmModule {
            name: flow.name.clone(),
            source: generate_flow_module(flow),
        })
        .collect();

    WasmProject {
        cargo_toml: generate_wasm_cargo_toml(),
        modules,
    }
}

fn generate_wasm_cargo_toml() -> String {
    r#"[package]
name = "axis-wasm"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
wit-bindgen = "0.36"

[profile.release]
opt-level = "s"
lto = true
"#
    .into()
}

fn generate_flow_module(flow: &FlowDef) -> String {
    let mut out = String::new();

    writeln!(out, "use serde::{{Deserialize, Serialize}};").unwrap();
    writeln!(out, "use serde_json::Value;").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "#[derive(Serialize)]").unwrap();
    writeln!(out, "struct Response {{").unwrap();
    writeln!(out, "    status: u16,").unwrap();
    writeln!(out, "    body: Value,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    if let Some(body) = &flow.body {
        writeln!(out, "#[derive(Deserialize)]").unwrap();
        writeln!(out, "struct RequestBody {{").unwrap();
        for f in &body.fields {
            writeln!(out, "    {}: Value,", f.name).unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    writeln!(out, "pub fn handle(request: &str) -> String {{").unwrap();
    writeln!(
        out,
        "    let _req: Value = serde_json::from_str(request).unwrap_or_default();"
    )
    .unwrap();
    writeln!(out).unwrap();

    for step in &flow.steps {
        match step {
            FlowStep::Let(l) => {
                writeln!(out, "    // LET {}", l.name).unwrap();
                writeln!(out, "    let {} = Value::Null;", l.name).unwrap();
            }
            FlowStep::Guard(g) => {
                writeln!(out, "    // GUARD {}", g.name).unwrap();
            }
            FlowStep::Insert(i) => {
                writeln!(out, "    // INSERT {}", i.source).unwrap();
            }
            FlowStep::Update(u) => {
                writeln!(out, "    // UPDATE {}", u.source).unwrap();
            }
            FlowStep::Delete(d) => {
                writeln!(out, "    // DELETE {}", d.source).unwrap();
            }
            _ => {}
        }
    }

    let code = flow.return_stmt.code;
    match &flow.return_stmt.body {
        Some(ReturnBody::Binding(name)) => {
            writeln!(
                out,
                "    let resp = Response {{ status: {code}, body: {name} }};"
            )
            .unwrap();
        }
        _ => {
            writeln!(
                out,
                "    let resp = Response {{ status: {code}, body: Value::Null }};"
            )
            .unwrap();
        }
    }
    writeln!(out, "    serde_json::to_string(&resp).unwrap()").unwrap();
    writeln!(out, "}}").unwrap();

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::compile_source;

    #[test]
    fn test_generate_wasm_project() {
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

FLOW list_users get /users
  AUTH session
  LET users
    QUERY users
  RETURN 200 users
"#;
        let program = compile_source(input).unwrap();
        let project = generate(&program);

        assert!(project.cargo_toml.contains("cdylib"));
        assert_eq!(project.modules.len(), 2);
        assert_eq!(project.modules[0].name, "get_user");
        assert!(project.modules[0].source.contains("pub fn handle"));
        assert_eq!(project.modules[1].name, "list_users");
    }

    #[test]
    fn test_wasm_module_with_body() {
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

        assert_eq!(project.modules.len(), 1);
        assert!(project.modules[0].source.contains("RequestBody"));
    }
}
