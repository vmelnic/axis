use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut file_args = Vec::new();
    let mut mode = Mode::Check;
    let mut project_mode = false;
    let mut named_args: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            }
            "--fmt" => mode = Mode::Format,
            "--sql" => mode = Mode::Sql,
            "--routes" => mode = Mode::Routes,
            "--emit" => mode = Mode::Emit,
            "--openapi" => mode = Mode::OpenApi,
            "--plan" => mode = Mode::Plan,
            "--link" => mode = Mode::Link,
            "--constrain" => mode = Mode::Constrain,
            "--logit-masks" => mode = Mode::LogitMasks,
            "--ts" => mode = Mode::Typescript,
            "--project" => project_mode = true,
            "--diff" => mode = Mode::Diff,
            "--completions" => mode = Mode::Completions,
            "--lsp" => mode = Mode::Lsp,
            "--watch" => mode = Mode::Watch,
            "--rust" => mode = Mode::Rust,
            "--testgen" => mode = Mode::TestGen,
            "--observability" => mode = Mode::Observability,
            "--migrate" => mode = Mode::Migrate,
            "--client-ts" => mode = Mode::ClientTs,
            "--client-rust" => mode = Mode::ClientRust,
            "--graphql" => mode = Mode::GraphQl,
            "--deploy" => mode = Mode::Deploy,
            "--serve" => mode = Mode::Serve,
            "--migrate-runner" => mode = Mode::MigrateRunner,
            s if s.starts_with("--") && s.contains('=') => {
                let (k, v) = s.split_once('=').unwrap();
                named_args.insert(k[2..].to_string(), v.to_string());
            }
            s if s.starts_with("--") => {
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    named_args.insert(s[2..].to_string(), args[i + 1].clone());
                    i += 1;
                } else {
                    eprintln!("unknown flag: {s}");
                    eprintln!(
                        "usage: axis [--fmt|--sql|--routes|--emit|--plan|--link|--rust|--serve|--project|--lsp|--watch|--migrate] <file|dir>"
                    );
                    std::process::exit(1);
                }
            }
            _ => file_args.push(arg.clone()),
        }
        i += 1;
    }
    let file_arg = file_args.first().cloned();

    if matches!(mode, Mode::Constrain) {
        let grammar = axis::constrain::export_grammar();
        println!("{}", serde_json::to_string_pretty(&grammar).unwrap());
        return;
    }

    if matches!(mode, Mode::LogitMasks) {
        let vocab = axis::constrain::token_vocabulary();
        let masks = axis::constrain::export_logit_masks();
        let output = serde_json::json!({
            "vocabulary": vocab,
            "masks": masks,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return;
    }

    if matches!(mode, Mode::Lsp) {
        axis::lsp::run_lsp().unwrap_or_else(|e| {
            eprintln!("lsp error: {e}");
            std::process::exit(1);
        });
        return;
    }

    if matches!(mode, Mode::Watch) {
        let dir = file_arg.clone().unwrap_or_else(|| ".".to_string());
        axis::watch::watch_project(std::path::Path::new(&dir)).unwrap_or_else(|e| {
            eprintln!("watch error: {e}");
            std::process::exit(1);
        });
        return;
    }

    if matches!(mode, Mode::Serve) {
        let dir = file_arg.clone().unwrap_or_else(|| ".".to_string());
        let port: u16 = named_args
            .get("port")
            .and_then(|p| p.parse().ok())
            .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
            .unwrap_or(3000);
        let opts = axis::serve::ServeOpts {
            src_dir: named_args
                .get("src")
                .cloned()
                .or_else(|| std::env::var("AXIS_SRC_DIR").ok()),
            adapters_dir: named_args
                .get("adapters")
                .cloned()
                .or_else(|| std::env::var("AXIS_ADAPTERS_DIR").ok()),
            templates_dir: named_args
                .get("templates")
                .cloned()
                .or_else(|| std::env::var("AXIS_TEMPLATES_DIR").ok()),
            locales_dir: named_args
                .get("locales")
                .cloned()
                .or_else(|| std::env::var("AXIS_LOCALES_DIR").ok()),
            db_url: named_args
                .get("db")
                .cloned()
                .or_else(|| std::env::var("DATABASE_URL").ok()),
            jwt_secret: named_args
                .get("jwt-secret")
                .cloned()
                .or_else(|| std::env::var("JWT_SECRET").ok()),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            axis::serve::serve(std::path::Path::new(&dir), port, opts)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("serve error: {e}");
                    std::process::exit(1);
                });
        });
        return;
    }

    let project_program = if project_mode {
        let dir = file_arg.clone().unwrap_or_else(|| ".".to_string());
        let dir_path = std::path::Path::new(&dir);
        let compile_dir_buf = named_args
            .get("src")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("AXIS_SRC_DIR")
                    .ok()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| {
                let src = dir_path.join("src");
                if src.exists() {
                    src
                } else {
                    dir_path.to_path_buf()
                }
            });
        let result = axis::project::compile_project(&compile_dir_buf);
        for e in &result.errors {
            eprintln!("ERROR [{}]: {}", e.file.display(), e.error);
        }
        if !result.is_ok() {
            std::process::exit(1);
        }
        if matches!(mode, Mode::Format) {
            eprintln!("--fmt cannot rewrite a multi-file project; format individual .axis files");
            std::process::exit(1);
        }
        Some(result.program)
    } else {
        None
    };

    if matches!(mode, Mode::Diff) {
        if file_args.len() != 2 {
            eprintln!("usage: axis --diff <old.axis> <new.axis>");
            std::process::exit(1);
        }
        let old_src = std::fs::read_to_string(&file_args[0]).unwrap_or_else(|e| {
            eprintln!("error: cannot read {}: {e}", file_args[0]);
            std::process::exit(1);
        });
        let new_src = std::fs::read_to_string(&file_args[1]).unwrap_or_else(|e| {
            eprintln!("error: cannot read {}: {e}", file_args[1]);
            std::process::exit(1);
        });
        let old = axis::project::compile_source(&old_src).unwrap_or_else(|e| {
            eprintln!("error parsing {}: {e}", file_args[0]);
            std::process::exit(1);
        });
        let new = axis::project::compile_source(&new_src).unwrap_or_else(|e| {
            eprintln!("error parsing {}: {e}", file_args[1]);
            std::process::exit(1);
        });
        let result = axis::diff::diff_programs(&old, &new, "v1", "v2");
        if result.has_changes() {
            print!("{}", axis::diff::format_all_migrations(&result));
        } else {
            println!("no changes");
        }
        return;
    }

    if matches!(mode, Mode::Completions) {
        let input = if let Some(path) = &file_arg {
            std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("error: cannot read {}: {e}", path);
                std::process::exit(1);
            })
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .unwrap_or_else(|e| {
                    eprintln!("error: cannot read stdin: {e}");
                    std::process::exit(1);
                });
            buf
        };
        let result = axis::incremental::validate_partial(&input);
        eprintln!("state: {:?}", result.state);
        eprintln!("constructs: {}", result.constructs_so_far);
        eprintln!("complete: {}", result.complete);
        for e in &result.errors {
            eprintln!("error line {}: {}", e.line, e.message);
        }
        println!("valid next: {}", result.valid_next.token_names().join(", "));
        return;
    }

    let program = project_program.unwrap_or_else(|| {
        let input = if let Some(path) = file_arg {
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("error: cannot read {}: {e}", path);
                std::process::exit(1);
            })
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .unwrap_or_else(|e| {
                    eprintln!("error: cannot read stdin: {e}");
                    std::process::exit(1);
                });
            buf
        };

        let mut lexer = axis::lexer::Lexer::new(&input);
        let tokens = lexer.tokenize().unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        let mut parser = axis::parser::Parser::new(tokens);
        parser.parse_program().unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        })
    });

    if matches!(mode, Mode::Format) {
        print!("{}", axis::fmt::format_program(&program));
        return;
    }

    let verifier = axis::verify::Verifier::new();
    let result = verifier.verify(&program);

    for e in &result.errors {
        eprintln!("ERROR: {e}");
    }
    for w in &result.warnings {
        eprintln!("WARN: {w}");
    }

    let shapes = program
        .constructs
        .iter()
        .filter(|c| matches!(c, axis::ast::Construct::Shape(_)))
        .count();
    let sources = program
        .constructs
        .iter()
        .filter(|c| matches!(c, axis::ast::Construct::Source(_)))
        .count();
    let realms = program
        .constructs
        .iter()
        .filter(|c| matches!(c, axis::ast::Construct::Realm(_)))
        .count();
    let flows = program
        .constructs
        .iter()
        .filter(|c| matches!(c, axis::ast::Construct::Flow(_)))
        .count();
    let streams = program
        .constructs
        .iter()
        .filter(|c| matches!(c, axis::ast::Construct::Stream(_)))
        .count();

    if result.error_count() > 0 {
        eprintln!(
            "{} shapes, {} sources, {} realms, {} flows, {} streams — {} errors",
            shapes,
            sources,
            realms,
            flows,
            streams,
            result.error_count()
        );
        std::process::exit(1);
    }

    match mode {
        Mode::Check => {
            println!(
                "OK: {} shapes, {} sources, {} realms, {} flows, {} streams — verified",
                shapes, sources, realms, flows, streams
            );
        }
        Mode::Sql => {
            let codegen = axis::codegens::sql::generate(&program);
            print!("{}", codegen.sql);
        }
        Mode::Routes => {
            let codegen = axis::codegens::sql::generate(&program);
            for route in &codegen.routes {
                print!("{route}");
            }
        }
        Mode::Emit => {
            let codegen = axis::codegens::sql::generate(&program);
            println!("-- SQL DDL\n{}", codegen.sql);
            println!("-- ROUTES");
            for route in &codegen.routes {
                print!("{route}");
            }
            if !codegen.migrations.is_empty() {
                println!("\n-- MIGRATIONS");
                for m in &codegen.migrations {
                    print!("{m}");
                }
            }
            if !codegen.sagas.is_empty() {
                println!("\n-- SAGAS");
                for s in &codegen.sagas {
                    print!("{s}");
                }
            }
            if !codegen.surfaces.is_empty() {
                println!("\n-- SURFACES");
                for s in &codegen.surfaces {
                    print!("{s}");
                }
            }
            if !codegen.streams.is_empty() {
                println!("\n-- STREAMS");
                for s in &codegen.streams {
                    println!("{} {} {}", s.transport, s.path, s.name);
                    for evt in &s.events {
                        let fields: Vec<String> = evt
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{n}: {t}"))
                            .collect();
                        println!("  EVENT {} [{}]", evt.name, fields.join(", "));
                    }
                }
            }
        }
        Mode::OpenApi => {
            let spec = axis::openapi::generate_openapi(&program);
            println!("{}", serde_json::to_string_pretty(&spec).unwrap());
        }
        Mode::Plan => {
            let plan_result = axis::plan::plan(&program);
            for w in &plan_result.warnings {
                eprintln!("PLAN WARN [{}]: {:?}: {}", w.flow, w.kind, w.message);
            }
            for fp in &plan_result.flow_plans {
                println!("FLOW {}", fp.name);
                for (i, group) in fp.execution_groups.iter().enumerate() {
                    let mode = if group.parallel {
                        "parallel"
                    } else {
                        "sequential"
                    };
                    println!("  group {i}: [{mode}] {}", group.steps.join(", "));
                }
                if let Some(txn) = &fp.transaction {
                    println!(
                        "  transaction: {} mutations, {} outbox effects",
                        txn.mutations.len(),
                        txn.outbox_effects.len()
                    );
                }
            }
            for sp in &plan_result.saga_plans {
                println!("SAGA {}", sp.name);
                for st in &sp.step_transactions {
                    let comp = if st.has_compensate {
                        "with compensate"
                    } else {
                        "no compensate"
                    };
                    println!("  step {}: {comp}", st.step_name);
                }
            }
        }
        Mode::Link => {
            let link_result = axis::link::link(&program);
            for e in &link_result.errors {
                eprintln!("LINK ERROR: {e}");
            }
            if link_result.is_ok() {
                println!("manifest: {}", link_result.manifest.hash);
                let sections: &[(&str, &[axis::link::HashedEntry])] = &[
                    ("shapes", &link_result.manifest.shapes),
                    ("sources", &link_result.manifest.sources),
                    ("realms", &link_result.manifest.realms),
                    ("policies", &link_result.manifest.policies),
                    ("services", &link_result.manifest.services),
                    ("flows", &link_result.manifest.flows),
                    ("sagas", &link_result.manifest.sagas),
                    ("surfaces", &link_result.manifest.surfaces),
                    ("migrations", &link_result.manifest.migrations),
                    ("streams", &link_result.manifest.streams),
                ];
                for (label, entries) in sections {
                    if !entries.is_empty() {
                        println!("  {label}:");
                        for entry in *entries {
                            print!("    {}: {}", entry.name, entry.hash);
                            if !entry.deps.is_empty() {
                                print!(" -> [{}]", entry.deps.join(", "));
                            }
                            println!();
                        }
                    }
                }
            } else {
                std::process::exit(1);
            }
        }
        Mode::Typescript => {
            let ts = axis::typescript::generate_typescript(&program);
            println!("// === types.ts ===");
            print!("{}", ts.types);
            println!("// === validators.ts ===");
            print!("{}", ts.validators);
            println!("// === queries.ts ===");
            print!("{}", ts.queries);
            println!("// === handlers.ts ===");
            print!("{}", ts.handlers);
            println!("// === router.ts ===");
            print!("{}", ts.router);
        }
        Mode::Rust => {
            let project = axis::codegens::rust::generate(&program);
            println!("// === Cargo.toml ===");
            print!("{}", project.cargo_toml);
            println!("// === src/main.rs ===");
            print!("{}", project.main_rs);
            println!("// === schema.sql ===");
            print!("{}", project.sql_schema);
        }
        Mode::TestGen => {
            let suite = axis::testgen::generate_tests(&program);
            println!("{}", serde_json::to_string_pretty(&suite).unwrap());
        }
        Mode::Observability => {
            let schema = axis::observability::generate_observability(&program);
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            println!(
                "\n{}",
                axis::observability::export_prometheus_config(&schema)
            );
        }
        Mode::ClientTs => {
            let client = axis::codegens::client::generate_client(&program);
            print!("{}", client.typescript);
        }
        Mode::ClientRust => {
            let client = axis::codegens::client::generate_client(&program);
            print!("{}", client.rust);
        }
        Mode::Deploy => {
            let deploy = axis::deploy::generate_deploy(&program);
            println!("// === Dockerfile ===");
            print!("{}", deploy.dockerfile);
            println!("// === docker-compose.yml ===");
            print!("{}", deploy.compose);
            println!("// === k8s.yaml ===");
            print!("{}", deploy.kubernetes);
            println!("// === .env ===");
            print!("{}", deploy.env_template);
        }
        Mode::GraphQl => {
            let schema = axis::graphql::generate_graphql(&program);
            print!("{schema}");
        }
        Mode::Migrate => {
            let plan = axis::migrate::plan_migrations(&program);
            if plan.migrations.is_empty() {
                println!("no migrations");
            } else {
                let files = axis::migrate::format_migration_files(&plan);
                for (filename, content) in &files {
                    println!("-- {filename}");
                    print!("{content}");
                    println!();
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plan.migrations).unwrap()
                );
            }
        }
        Mode::MigrateRunner => {
            let plan = axis::migrate::plan_migrations(&program);
            let runner = axis::migrate::generate_runner(&plan);
            println!("// === Cargo.toml ===");
            print!("{}", runner.cargo_toml);
            println!("// === src/main.rs ===");
            print!("{}", runner.main_rs);
        }
        Mode::Format
        | Mode::Constrain
        | Mode::LogitMasks
        | Mode::Diff
        | Mode::Completions
        | Mode::Lsp
        | Mode::Watch
        | Mode::Serve => unreachable!(),
    }
}

fn print_help() {
    println!(
        r#"axis — backend programming language

USAGE:
    axis [MODE] [OPTIONS] <file|dir>

MODES:
    (default)         Parse and verify a single .axis file
    --project         Compile a multi-file project directory
    --serve           Start HTTP server (interprets AST at runtime)
    --sql             Generate SQL DDL
    --fmt             Format source
    --routes          List routes
    --openapi         Generate OpenAPI 3.1 spec
    --ts              Generate TypeScript types
    --rust            Generate Rust/axum server
    --graphql         Generate GraphQL schema
    --client-ts       Generate TypeScript client SDK
    --client-rust     Generate Rust client SDK
    --emit            Emit JSON execution plan
    --plan            Show execution plan with warnings
    --link            Show cross-reference links
    --migrate         Generate migration SQL
    --migrate-runner  Generate migration runner
    --testgen         Generate test cases
    --deploy          Generate deployment manifests
    --observability   Generate observability config
    --constrain       Export grammar state machine (JSON)
    --logit-masks     Export logit masks
    --diff            Structural diff between two files
    --lsp             Start language server
    --watch           Watch directory and recompile on change

OPTIONS:
    --port <N>        Server port (default: 3000, env: PORT)
    --src <dir>       Source directory for .axis files (default: <dir>/src or <dir>)
    --adapters <dir>  Adapters directory (default: <dir>/adapters, env: AXIS_ADAPTERS_DIR)
    --templates <dir> Templates directory (default: <dir>/templates, env: AXIS_TEMPLATES_DIR)
    --locales <dir>   Locales directory (default: <dir>/locales, env: AXIS_LOCALES_DIR)
    --db <url>        Database URL (postgres://, mysql://, sqlite:, env: DATABASE_URL)
    --jwt-secret <s>  JWT signing secret (env: JWT_SECRET)
    -h, --help        Show this help

ENVIRONMENT:
    DATABASE_URL         Database connection string (postgres://, mysql://, sqlite:)
    JWT_SECRET           JWT signing secret
    PORT                 Server port
    AXIS_SRC_DIR         Source directory override
    AXIS_ADAPTERS_DIR    Adapters directory override
    AXIS_TEMPLATES_DIR   Templates directory override
    AXIS_LOCALES_DIR     Locales directory override
    DEFAULT_LOCALE       Default locale code (default: en)

EXAMPLES:
    axis app.axis                         Check single file
    axis --project myapp/                 Compile project
    axis --serve myapp/                   Serve on port 3000
    axis --serve --port 8080 myapp/       Serve on port 8080
    axis --project --sql myapp/           Generate DDL
"#
    );
}

enum Mode {
    Check,
    Format,
    Sql,
    Routes,
    OpenApi,
    Emit,
    Plan,
    Link,
    Constrain,
    Typescript,
    Diff,
    Completions,
    Lsp,
    Watch,
    Rust,
    LogitMasks,
    TestGen,
    Observability,
    Migrate,
    ClientTs,
    ClientRust,
    GraphQl,
    Deploy,
    MigrateRunner,
    Serve,
}
