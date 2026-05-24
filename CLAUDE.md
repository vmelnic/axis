# Axis — Claude Code Instructions

## What Axis is

A standalone backend API programming language. Not part of SOMA. Not a config format. A language with 12 top-level constructs that fully specify a production backend. The binary both compiles (generates artifacts) and serves (interprets AST as a live HTTP server).

Designed for small LLMs (1B/3B/7B) to write correctly: indentation-based, prefix-only expressions, LL(1) grammar, constrained keyword vocabulary.

## Build and test

```bash
cargo test          # must pass after every change
cargo clippy        # must be clean (0 warnings)
```

## Repository structure

```
src/
  compiler/         # Compile pipeline front-end
    token.rs        # Token and Span types
    lexer.rs        # Tokenizer — indentation-sensitive, produces Token stream
    ast.rs          # All AST types — ShapeDef, SourceDef, FlowDef, StreamDef, etc.
    parser.rs       # LL(1) recursive descent → AST (Program with 12 Construct variants)
    error.rs        # Error types (LexError, ParseError, TypeError, etc.)
    link.rs         # Cross-reference linking (shapes ↔ sources ↔ flows)
    verify.rs       # Semantic verification (types, capabilities, tenants, policies)
    plan.rs         # Execution planning with warnings
  codegens/         # Code generation backends
    sql.rs          # SQL DDL generation + IR instruction lowering
    rust.rs         # Rust/axum server generation from AST
    wasm.rs         # WASM module stub generation for flows
    client.rs       # Client SDK generation (TypeScript + Rust API clients)
  generators/       # Artifact generators (non-code output)
    openapi.rs      # OpenAPI 3.0 spec generation
    typescript.rs   # TypeScript server-side type generation
    graphql.rs      # GraphQL schema generation
    deploy.rs       # Deployment manifests (Dockerfile, Compose, K8s, .env)
    migrate.rs      # Database migration planning with up/down SQL
    testgen.rs      # Test case generation from flows/sagas/streams
    observability.rs # Observability schema (metrics, traces, logs, Prometheus config)
  editor/           # Editor and tooling support
    lsp.rs          # Language server (completions, go-to-def, references, hover, rename, symbols)
    incremental.rs  # Incremental parsing for editor support
    diff.rs         # Structural diff between two axis programs
    fmt.rs          # Source formatter
    constrain.rs    # Grammar state machine export for LLM token guidance
  ports/            # Non-SQL backend adapters
    adapter.rs      # SourceAdapter trait + AdapterRegistry
    redis.rs        # Redis port (feature-gated: redis-port)
  project.rs        # Multi-file project compilation
  watch.rs          # File watcher with debounced recompilation + codegen hot reload
  serve.rs          # HTTP server — interprets AST at runtime (axum + sqlx, all 12 constructs)
  wasm.rs           # WASM escape hatch loader (module registry, manifest)
  config.rs         # Configuration types
  main.rs           # CLI entry point
  lib.rs            # Module tree + re-exports
examples/
  booking.axis      # Booking platform example (shapes, sources, realms, flows)
  full.axis         # 10 of 12 constructs exercised (missing FUNC, STORAGE)
projects/
  todolist/         # Basic CRUD proof (20/20 integration tests)
  advanced/         # Full-feature proof: auth, tenants, guards, rules, funcs, match,
                    #   try/recover, rate limiting, caching, surfaces (32/32 tests)
  helperbook/       # Production proof: client-provider marketplace
                    #   (run `axis --project helperbook/` for current counts)
tests/
  integration.rs    # Integration tests
editors/
  vscode/           # VS Code extension (syntax highlighting + LSP client)
```

## The 12 constructs

SHAPE, SOURCE, REALM, FLOW, SAGA, SURFACE, POLICY, SERVICE, MIGRATE, STREAM, FUNC, STORAGE.

Every parse, verify, codegen, and emit path must handle all 12. If you add logic for one, check the other 11.

## AST field names — do not guess

These are the actual field names. Read `src/ast.rs` if unsure.

- `TypeExpr` — not `FieldType`, not `ScalarType`
- `SourceDef.source_type: SourceType` — not `driver`
- `CapabilityDef.target: String` — not `source`
- `AuthDecl` — an enum, not a struct
- `SagaStep.flow_steps` — not `actions`
- `RouteDef.target` — not `flow`

## Pipelines

Compile pipeline (artifact generation):
```
Source → Lexer → Parser → Linker → Verifier → Planner → Codegen → Emit
```

Serve pipeline (live HTTP server):
```
Source → Lexer → Parser → Verifier → Serve (axum interprets AST, sqlx talks to DB)
```

Each stage is independent and testable. The pipeline is NOT circular — each stage consumes the output of the previous one.

## Multi-database support

SOURCE accepts POSTGRES, MYSQL, or SQLITE — all three have full runtime support (serve.rs) and dialect-aware DDL (codegens/sql.rs, generators/migrate.rs). REDIS is supported via the `SourceAdapter` trait (ports/adapter.rs) with a concrete Redis port (ports/redis.rs, feature-gated). ELASTICSEARCH, DYNAMODB are parsed and verified but have no runtime port yet. SHAPE is database-agnostic — the same shape works with any backend.

Runtime (`serve.rs`): `DbBackend` enum wraps PgPool/MySqlPool/SqlitePool. `Dialect` enum drives placeholder style ($1 vs ?), RETURNING support, and type mappings. MySQL lacks RETURNING * — INSERT uses client-side UUID + SELECT back. Per-source pools: `AppState.dbs: HashMap<String, DbBackend>`, each SOURCE gets its own pool via `{NAME}_DATABASE_URL` env var (fallback to `DATABASE_URL`). Redis sources connect via `{NAME}_REDIS_URL` (fallback `REDIS_URL`).

Codegen (`codegens/sql.rs`, `generators/migrate.rs`): `SqlDialect`/`SourceType` drives DDL type mappings per source. Each source gets dialect-appropriate DDL (UUID→CHAR(36)/TEXT, JSONB→JSON/TEXT, TIMESTAMPTZ→DATETIME/TEXT, etc.). GIN/GiST indexes fall back to regular indexes on MySQL/SQLite.

Generated server (`codegens/rust.rs`): Multi-dialect — generates per-source pool fields (`db_{source}: PgPool/MySqlPool/SqlitePool`), dialect-correct placeholders, RETURNING handling, and NOW() expressions. Only includes sqlx features for backends actually used.

Ports (`ports/`): `SourceAdapter` trait (ports/adapter.rs) with `fetch/query/insert/update/delete` for non-SQL sources. `AdapterRegistry` maps source types to factories. Redis port (ports/redis.rs) implements FETCH/INSERT/DELETE via native Redis protocol. Enabled with `--features redis-port`.

## Key invariants

- **2-space indentation only.** The lexer emits Indent/Dedent tokens based on 2-space levels.
- **Prefix expressions.** `EQ a b`, never `a == b`. Operator always comes first.
- **LL(1) parsing.** One token of lookahead. No backtracking. If the grammar needs backtracking, redesign the syntax.
- **No application logic in the compiler.** The compiler knows Axis constructs, never domain-specific behavior.
- **Tests are the proof.** Every new feature needs tests. "Compiles" is not "works."

## Working rules

- Read `src/compiler/ast.rs` before writing any code that touches AST types.
- Run `cargo test` and `cargo clippy` after every change.
- Don't add intermediate formats or indirection layers. Direct transforms: AST → output.
- Don't add features the grammar doesn't support yet. Extend grammar first, then the pipeline.
- Comments in generated output (SQL, OpenAPI, TypeScript) are fine. Comments in Axis source use `--`.
