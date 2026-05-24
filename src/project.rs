use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::error::AxisError;

pub struct ProjectResult {
    pub program: Program,
    pub files: Vec<ProjectFile>,
    pub errors: Vec<ProjectError>,
}

pub struct ProjectFile {
    pub path: PathBuf,
    pub construct_count: usize,
}

pub struct ProjectError {
    pub file: PathBuf,
    pub error: String,
}

impl ProjectResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn compile_project(dir: &Path) -> ProjectResult {
    let mut all_constructs = Vec::new();
    let mut files = Vec::new();
    let mut errors = Vec::new();

    let mut paths = collect_axis_files(dir);
    paths.sort();

    for path in &paths {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                errors.push(ProjectError {
                    file: path.clone(),
                    error: format!("cannot read: {e}"),
                });
                continue;
            }
        };

        let mut lexer = crate::lexer::Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                errors.push(ProjectError {
                    file: path.clone(),
                    error: format!("{e}"),
                });
                continue;
            }
        };

        let mut parser = crate::parser::Parser::new(tokens);
        match parser.parse_program() {
            Ok(prog) => {
                let count = prog.constructs.len();
                files.push(ProjectFile {
                    path: path.clone(),
                    construct_count: count,
                });
                all_constructs.extend(prog.constructs);
            }
            Err(e) => {
                errors.push(ProjectError {
                    file: path.clone(),
                    error: format!("{e}"),
                });
            }
        }
    }

    ProjectResult {
        program: Program { constructs: all_constructs },
        files,
        errors,
    }
}

pub fn compile_files(paths: &[PathBuf]) -> ProjectResult {
    let mut all_constructs = Vec::new();
    let mut files = Vec::new();
    let mut errors = Vec::new();

    for path in paths {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                errors.push(ProjectError {
                    file: path.clone(),
                    error: format!("cannot read: {e}"),
                });
                continue;
            }
        };

        match compile_source(&source) {
            Ok(prog) => {
                let count = prog.constructs.len();
                files.push(ProjectFile {
                    path: path.clone(),
                    construct_count: count,
                });
                all_constructs.extend(prog.constructs);
            }
            Err(e) => {
                errors.push(ProjectError {
                    file: path.clone(),
                    error: format!("{e}"),
                });
            }
        }
    }

    ProjectResult {
        program: Program { constructs: all_constructs },
        files,
        errors,
    }
}

pub fn compile_source(source: &str) -> Result<Program, AxisError> {
    let mut lexer = crate::lexer::Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = crate::parser::Parser::new(tokens);
    parser.parse_program()
}

pub fn collect_axis_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(collect_axis_files(&path));
            } else if path.extension().is_some_and(|e| e == "axis") {
                result.push(path);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_compile_single_file() {
        let paths = vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
        ];
        let result = compile_files(&paths);
        assert!(result.is_ok());
        assert_eq!(result.files.len(), 1);
        assert!(!result.program.constructs.is_empty());
    }

    #[test]
    fn test_compile_multiple_files() {
        let paths = vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/booking.axis"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/full.axis"),
        ];
        let result = compile_files(&paths);
        assert!(result.is_ok());
        assert_eq!(result.files.len(), 2);

        let booking_only = compile_files(&paths[..1]);
        let full_only = compile_files(&paths[1..]);
        assert_eq!(
            result.program.constructs.len(),
            booking_only.program.constructs.len() + full_only.program.constructs.len()
        );
    }

    #[test]
    fn test_compile_project_dir() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
        let result = compile_project(&dir);
        assert!(result.is_ok());
        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn test_compile_split_files() {
        let tmp = tempdir();
        fs::write(tmp.join("shapes.axis"), r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#).unwrap();
        fs::write(tmp.join("sources.axis"), r#"SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#).unwrap();
        fs::write(tmp.join("flows.axis"), r#"REALM api
  CAPABILITY read users

FLOW get_user get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#).unwrap();

        let result = compile_project(&tmp);
        assert!(result.is_ok(), "errors: {:?}", result.errors.iter().map(|e| &e.error).collect::<Vec<_>>());
        assert_eq!(result.files.len(), 3);

        let shapes = result.program.constructs.iter()
            .filter(|c| matches!(c, crate::ast::Construct::Shape(_))).count();
        let sources = result.program.constructs.iter()
            .filter(|c| matches!(c, crate::ast::Construct::Source(_))).count();
        let flows = result.program.constructs.iter()
            .filter(|c| matches!(c, crate::ast::Construct::Flow(_))).count();
        assert_eq!(shapes, 1);
        assert_eq!(sources, 1);
        assert_eq!(flows, 1);
    }

    #[test]
    fn test_compile_bad_file() {
        let tmp = tempdir();
        fs::write(tmp.join("good.axis"), "SHAPE Foo\n  id UUID PK AUTO\n").unwrap();
        fs::write(tmp.join("bad.axis"), "THIS IS NOT VALID AXIS\n").unwrap();

        let result = compile_project(&tmp);
        assert!(!result.is_ok());
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].file.ends_with("bad.axis"));
    }

    #[test]
    fn test_compile_missing_file() {
        let paths = vec![PathBuf::from("/nonexistent/path.axis")];
        let result = compile_files(&paths);
        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_compile_empty_dir() {
        let tmp = tempdir();
        let result = compile_project(&tmp);
        assert!(result.is_ok());
        assert_eq!(result.files.len(), 0);
        assert!(result.program.constructs.is_empty());
    }

    #[test]
    fn test_split_project_verifies() {
        let tmp = tempdir();
        fs::write(tmp.join("shapes.axis"), r#"SHAPE User
  id UUID PK AUTO
  name STRING 100 REQUIRED
"#).unwrap();
        fs::write(tmp.join("sources.axis"), r#"SOURCE users POSTGRES
  SHAPE User
  INDEX id
"#).unwrap();
        fs::write(tmp.join("realms.axis"), r#"REALM api
  CAPABILITY read users
"#).unwrap();
        fs::write(tmp.join("flows.axis"), r#"FLOW get_user get /users/:id
  REALM api
  AUTH session
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
"#).unwrap();

        let result = compile_project(&tmp);
        assert!(result.is_ok());

        let verifier = crate::verify::Verifier::new();
        let verify_result = verifier.verify(&result.program);
        assert!(verify_result.is_ok(), "verify errors: {:?}",
            verify_result.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>());
    }

    fn tempdir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("axis_test_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
