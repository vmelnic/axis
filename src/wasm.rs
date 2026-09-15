use serde::Serialize;

use crate::ast::*;

#[derive(Debug, Serialize)]
pub struct WasmManifest {
    pub modules: Vec<WasmModuleRef>,
}

#[derive(Debug, Serialize)]
pub struct WasmModuleRef {
    pub hash: String,
    pub used_by: Vec<String>,
    pub inputs: Vec<String>,
}

pub fn collect_wasm_refs(program: &Program) -> WasmManifest {
    let mut modules: Vec<WasmModuleRef> = Vec::new();

    for construct in &program.constructs {
        if let Construct::Flow(f) = construct {
            collect_from_steps(&f.steps, &f.name, &mut modules);
        }
        if let Construct::Saga(s) = construct {
            for step in &s.steps {
                collect_from_steps(&step.flow_steps, &s.name, &mut modules);
            }
        }
    }

    WasmManifest { modules }
}

fn collect_from_steps(steps: &[FlowStep], flow_name: &str, modules: &mut Vec<WasmModuleRef>) {
    for step in steps {
        match step {
            FlowStep::Let(l) => collect_from_expr(&l.expr, flow_name, modules),
            FlowStep::Match(m) => {
                for branch in &m.branches {
                    collect_from_steps(&branch.steps, flow_name, modules);
                }
                if let Some(default) = &m.default {
                    collect_from_steps(default, flow_name, modules);
                }
            }
            _ => {}
        }
    }
}

fn collect_from_expr(expr: &Expr, flow_name: &str, modules: &mut Vec<WasmModuleRef>) {
    if let Expr::WasmCall { hash, inputs } = expr {
        if let Some(existing) = modules.iter_mut().find(|m| m.hash == *hash) {
            if !existing.used_by.contains(&flow_name.to_string()) {
                existing.used_by.push(flow_name.to_string());
            }
        } else {
            modules.push(WasmModuleRef {
                hash: hash.clone(),
                used_by: vec![flow_name.to_string()],
                inputs: inputs.clone(),
            });
        }
    }
    if let Expr::Cached { expr, .. } = expr {
        collect_from_expr(expr, flow_name, modules);
    }
}

pub fn generate_loader_code() -> String {
    r#"use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use wasmtime::*;

pub struct WasmRegistry {
    engine: Engine,
    modules: HashMap<String, Module>,
}

impl WasmRegistry {
    pub fn new() -> Self {
        let engine = Engine::default();
        Self { engine, modules: HashMap::new() }
    }

    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, String> {
        let mut count = 0;
        let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                let hash = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let module = Module::from_file(&self.engine, &path)
                    .map_err(|e| format!("cannot load {}: {e}", path.display()))?;
                self.modules.insert(hash, module);
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn load_module(&mut self, hash: &str, bytes: &[u8]) -> Result<(), String> {
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| format!("cannot compile wasm {hash}: {e}"))?;
        self.modules.insert(hash.to_string(), module);
        Ok(())
    }

    pub fn call(&self, hash: &str, inputs: &[serde_json::Value]) -> Result<serde_json::Value, String> {
        let module = self.modules.get(hash)
            .ok_or_else(|| format!("wasm module not found: {hash}"))?;

        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, module, &[])
            .map_err(|e| format!("wasm instantiation failed: {e}"))?;

        let input_json = serde_json::to_string(inputs).unwrap();

        let memory = instance.get_memory(&mut store, "memory")
            .ok_or("wasm module has no memory export")?;

        let alloc = instance.get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| format!("wasm module missing alloc: {e}"))?;

        let process = instance.get_typed_func::<(i32, i32), i32>(&mut store, "process")
            .map_err(|e| format!("wasm module missing process: {e}"))?;

        let input_bytes = input_json.as_bytes();
        let ptr = alloc.call(&mut store, input_bytes.len() as i32)
            .map_err(|e| format!("alloc failed: {e}"))?;

        memory.data_mut(&mut store)[ptr as usize..ptr as usize + input_bytes.len()]
            .copy_from_slice(input_bytes);

        let result_ptr = process.call(&mut store, (ptr, input_bytes.len() as i32))
            .map_err(|e| format!("process failed: {e}"))?;

        let result_len_bytes = &memory.data(&store)[result_ptr as usize..result_ptr as usize + 4];
        let result_len = i32::from_le_bytes(result_len_bytes.try_into().unwrap()) as usize;
        let result_data = &memory.data(&store)[result_ptr as usize + 4..result_ptr as usize + 4 + result_len];
        let result_str = std::str::from_utf8(result_data)
            .map_err(|e| format!("invalid utf8 from wasm: {e}"))?;

        serde_json::from_str(result_str)
            .map_err(|e| format!("invalid json from wasm: {e}"))
    }

    pub fn has_module(&self, hash: &str) -> bool {
        self.modules.contains_key(hash)
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}
"#.to_string()
}

pub fn generate_runtime_wasm_handler() -> String {
    r#"            Instruction::WasmCall { binding, hash, inputs } => {
                let input_values: Vec<Value> = inputs.iter().map(|i| {
                    resolve_value_json(i, &bindings)
                }).collect();
                let result = wasm_registry.call(hash, &input_values)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e})))?;
                bindings.insert(binding.clone(), result);
            }
"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::compile_source;

    #[test]
    fn test_collect_no_wasm() {
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
"#;
        let program = compile_source(input).unwrap();
        let manifest = collect_wasm_refs(&program);
        assert!(manifest.modules.is_empty());
    }

    #[test]
    fn test_collect_wasm_refs() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW process_user get /users/:id/process
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET result
    CALL wasm sha256:abc123 username
  RETURN 200 result
"#;
        let program = compile_source(input).unwrap();
        let manifest = collect_wasm_refs(&program);
        assert_eq!(manifest.modules.len(), 1);
        assert_eq!(manifest.modules[0].hash, "sha256:abc123");
        assert_eq!(manifest.modules[0].used_by, vec!["process_user"]);
        assert_eq!(manifest.modules[0].inputs, vec!["username"]);
    }

    #[test]
    fn test_collect_shared_wasm() {
        let input = r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX id

FLOW process_a get /a/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET result
    CALL wasm sha256:abc123 username
  RETURN 200 result

FLOW process_b get /b/:id
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  LET result
    CALL wasm sha256:abc123 userid
  RETURN 200 result
"#;
        let program = compile_source(input).unwrap();
        let manifest = collect_wasm_refs(&program);
        assert_eq!(manifest.modules.len(), 1);
        assert_eq!(manifest.modules[0].used_by, vec!["process_a", "process_b"]);
    }

    #[test]
    fn test_loader_code_generated() {
        let code = generate_loader_code();
        assert!(code.contains("struct WasmRegistry"));
        assert!(code.contains("fn load_dir"));
        assert!(code.contains("fn call"));
        assert!(code.contains("fn load_module"));
        assert!(code.contains("wasmtime"));
    }

    #[test]
    fn test_runtime_handler_generated() {
        let code = generate_runtime_wasm_handler();
        assert!(code.contains("WasmCall"));
        assert!(code.contains("wasm_registry.call"));
        assert!(code.contains("binding"));
    }

    #[test]
    fn test_wasm_manifest_serializes() {
        let manifest = WasmManifest {
            modules: vec![WasmModuleRef {
                hash: "sha256:abc".into(),
                used_by: vec!["flow_a".into()],
                inputs: vec!["user.name".into()],
            }],
        };
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(json.contains("sha256:abc"));
        assert!(json.contains("flow_a"));
    }
}
