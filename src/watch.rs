use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::project;
use crate::verify::Verifier;

#[derive(Debug, Clone, Default)]
pub struct WatchOptions {
    pub emit_sql: bool,
    pub emit_typescript: bool,
    pub emit_openapi: bool,
    pub output_dir: Option<String>,
}

pub fn watch_project(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    watch_project_with_options(dir, &WatchOptions::default())
}

pub fn watch_project_with_options(dir: &Path, opts: &WatchOptions) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("watching {} for .axis changes...", dir.display());
    run_check(dir, opts);

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(event) = res {
            let dominated = matches!(
                event.kind,
                notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
            );
            if dominated && event.paths.iter().any(|p| {
                p.extension().is_some_and(|e| e == "axis")
            }) {
                let _ = tx.send(());
            }
        }
    })?;

    watcher.watch(dir, RecursiveMode::Recursive)?;

    let debounce = Duration::from_millis(200);
    let mut last_run = Instant::now() - debounce;

    while let Ok(()) = rx.recv() {
        while rx.try_recv().is_ok() {}
        let now = Instant::now();
        if now.duration_since(last_run) < debounce {
            std::thread::sleep(debounce - now.duration_since(last_run));
            while rx.try_recv().is_ok() {}
        }
        last_run = Instant::now();
        run_check(dir, opts);
    }
    Ok(())
}

fn run_check(dir: &Path, opts: &WatchOptions) {
    eprint!("\x1b[2J\x1b[H");

    let result = project::compile_project(dir);
    let mut error_count = 0;

    for e in &result.errors {
        eprintln!("\x1b[31mERROR\x1b[0m [{}]: {}", e.file.display(), e.error);
        error_count += 1;
    }

    if result.is_ok() {
        let verifier = Verifier::new();
        let verify_result = verifier.verify(&result.program);

        for e in &verify_result.errors {
            eprintln!("\x1b[31mERROR\x1b[0m: {e}");
            error_count += 1;
        }
        for w in &verify_result.warnings {
            eprintln!("\x1b[33mWARN\x1b[0m: {w}");
        }

        if error_count == 0 {
            eprintln!(
                "\x1b[32mOK\x1b[0m: {} files, {} constructs — verified",
                result.files.len(),
                result.program.constructs.len()
            );
            for f in &result.files {
                eprintln!("  {} ({} constructs)", f.path.display(), f.construct_count);
            }
            emit_artifacts(&result.program, opts);
        } else {
            eprintln!(
                "\n{} files, {} constructs — \x1b[31m{} errors\x1b[0m",
                result.files.len(),
                result.program.constructs.len(),
                error_count
            );
        }
    } else {
        eprintln!(
            "\n\x1b[31m{} parse errors\x1b[0m across {} files",
            error_count,
            result.files.len() + result.errors.len()
        );
    }

    let now = chrono_now();
    eprintln!("\n[{now}] watching for changes...");
}

fn emit_artifacts(program: &crate::ast::Program, opts: &WatchOptions) {
    let out_dir = opts.output_dir.as_deref().unwrap_or(".axis-out");
    let any_emit = opts.emit_sql || opts.emit_typescript || opts.emit_openapi;
    if !any_emit {
        return;
    }

    let out_path = Path::new(out_dir);
    if std::fs::create_dir_all(out_path).is_err() {
        eprintln!("\x1b[31mERROR\x1b[0m: cannot create output dir {out_dir}");
        return;
    }

    let mut emitted = Vec::new();

    if opts.emit_sql {
        let codegen = crate::codegens::sql::generate(program);
        if write_if_changed(out_path.join("schema.sql"), &codegen.sql) {
            emitted.push("schema.sql");
        }
    }

    if opts.emit_typescript {
        let ts = crate::typescript::generate_typescript(program);
        let mut combined = String::new();
        combined.push_str("// types.ts\n");
        combined.push_str(&ts.types);
        combined.push_str("\n// validators.ts\n");
        combined.push_str(&ts.validators);
        combined.push_str("\n// queries.ts\n");
        combined.push_str(&ts.queries);
        combined.push_str("\n// handlers.ts\n");
        combined.push_str(&ts.handlers);
        combined.push_str("\n// router.ts\n");
        combined.push_str(&ts.router);
        if write_if_changed(out_path.join("types.ts"), &ts.types) {
            emitted.push("types.ts");
        }
        if write_if_changed(out_path.join("generated.ts"), &combined) {
            emitted.push("generated.ts");
        }
    }

    if opts.emit_openapi {
        let spec = crate::openapi::generate_openapi(program);
        let json = serde_json::to_string_pretty(&spec).unwrap();
        if write_if_changed(out_path.join("openapi.json"), &json) {
            emitted.push("openapi.json");
        }
    }

    if !emitted.is_empty() {
        eprintln!("  \x1b[36memitted\x1b[0m: {}", emitted.join(", "));
    }
}

fn write_if_changed(path: std::path::PathBuf, content: &str) -> bool {
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == content {
            return false;
        }
    }
    std::fs::write(&path, content).is_ok()
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_run_check_valid() {
        let dir = tempdir();
        fs::write(dir.join("test.axis"), "SHAPE User\n  id UUID PK AUTO\n").unwrap();
        run_check(&dir, &WatchOptions::default());
    }

    #[test]
    fn test_run_check_invalid() {
        let dir = tempdir();
        fs::write(dir.join("bad.axis"), "NOT VALID AXIS\n").unwrap();
        run_check(&dir, &WatchOptions::default());
    }

    #[test]
    fn test_run_check_empty() {
        let dir = tempdir();
        run_check(&dir, &WatchOptions::default());
    }

    #[test]
    fn test_run_check_mixed() {
        let dir = tempdir();
        fs::write(dir.join("good.axis"), "SHAPE User\n  id UUID PK AUTO\n").unwrap();
        fs::write(dir.join("bad.axis"), "GARBAGE\n").unwrap();
        run_check(&dir, &WatchOptions::default());
    }

    #[test]
    fn test_emit_sql_artifacts() {
        let dir = tempdir();
        let out_dir = tempdir();
        let src = "SHAPE User\n  id UUID PK AUTO\n  name STRING 100 REQUIRED\n\nSOURCE users POSTGRES\n  SHAPE User\n  INDEX id\n";
        fs::write(dir.join("schema.axis"), src).unwrap();

        let opts = WatchOptions {
            emit_sql: true,
            output_dir: Some(out_dir.to_string_lossy().into()),
            ..Default::default()
        };
        run_check(&dir, &opts);

        assert!(out_dir.join("schema.sql").exists());
        let sql = fs::read_to_string(out_dir.join("schema.sql")).unwrap();
        assert!(sql.contains("CREATE TABLE"));
    }

    #[test]
    fn test_emit_openapi_artifacts() {
        let dir = tempdir();
        let out_dir = tempdir();
        let src = r#"SHAPE User
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
        fs::write(dir.join("api.axis"), src).unwrap();

        let opts = WatchOptions {
            emit_openapi: true,
            output_dir: Some(out_dir.to_string_lossy().into()),
            ..Default::default()
        };
        run_check(&dir, &opts);

        assert!(out_dir.join("openapi.json").exists());
        let json = fs::read_to_string(out_dir.join("openapi.json")).unwrap();
        assert!(json.contains("openapi"));
    }

    #[test]
    fn test_write_if_changed_dedup() {
        let dir = tempdir();
        let path = dir.join("test.txt");
        assert!(write_if_changed(path.clone(), "hello"));
        assert!(!write_if_changed(path.clone(), "hello"));
        assert!(write_if_changed(path.clone(), "world"));
    }

    #[test]
    fn test_no_emit_on_errors() {
        let dir = tempdir();
        let out_dir = tempdir();
        fs::write(dir.join("bad.axis"), "GARBAGE\n").unwrap();

        let opts = WatchOptions {
            emit_sql: true,
            output_dir: Some(out_dir.to_string_lossy().into()),
            ..Default::default()
        };
        run_check(&dir, &opts);

        assert!(!out_dir.join("schema.sql").exists());
    }

    fn tempdir() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("axis_watch_test_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
