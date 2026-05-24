# Axis

A backend API programming language designed for small LLMs (1B/3B/7B) to write correctly.

Axis uses indentation-based syntax, prefix-only expressions, and an LL(1) grammar with 12 top-level constructs that fully specify a production backend: data models, databases, endpoints, auth, permissions, distributed transactions, API surfaces, policies, migrations, real-time streams, and reusable functions.

The `axis` binary is self-contained — it parses `.axis` files, generates SQL DDL, and serves a full HTTP API directly from the AST. No code generation step, no external runtime. One binary, one source file. Supports PostgreSQL, MySQL, and SQLite — the backend auto-detects from the DATABASE_URL.

**[Full Documentation](docs/index.md)**

## Example

```
SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE

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
```

## Constructs

| Construct | Purpose |
|-----------|---------|
| `SHAPE` | Data model with typed fields, constraints, defaults |
| `SOURCE` | Database binding (Postgres, MySQL, SQLite, Redis) with indexes |
| `REALM` | Permission scope with capabilities and tenant isolation |
| `FLOW` | API endpoint — method, path, auth, guards, data ops, response |
| `SAGA` | Distributed transaction with compensating steps |
| `SURFACE` | API version/routing layer mapping routes to flows |
| `POLICY` | Cross-cutting rules enforced at compile time |
| `SERVICE` | External service integration |
| `MIGRATE` | Schema migration with ADD/DROP/RENAME operations |
| `STREAM` | Real-time events over WebSocket or SSE |
| `FUNC` | Reusable pure function with typed inputs/output |
| `STORAGE` | File storage backend (local or S3) with upload support |

## Two execution modes

**Compile** — generate artifacts (SQL, OpenAPI, TypeScript, Rust server, client SDKs, deployment manifests, migrations, test suites):

```
.axis source → lex → parse → link → verify → plan → codegen → emit
```

**Serve** — interpret the AST directly as a running HTTP server:

```
.axis source → lex → parse → verify → serve (axum + sqlx → Postgres/MySQL/SQLite)
```

The interpreter handles all 12 constructs at runtime: JWT/API-key auth, tenant isolation, guards, rules, rate limiting, response caching, MATCH/TRY/RECOVER control flow, EACH loops, FUNC calls, SURFACE route aliasing, WebSocket/SSE streams, SAGA compensation, and Prometheus metrics.

## CLI

```
axis <file>                  # check (parse + link + verify)
axis --serve <dir>           # serve HTTP API from .axis files (requires DATABASE_URL)
axis --sql <file>            # generate SQL DDL
axis --fmt <file>            # format
axis --routes <file>         # list routes
axis --emit <file>           # execution plan
axis --plan <file>           # execution plan with warnings
axis --link <file>           # link report
axis --openapi <file>        # OpenAPI 3.0 spec
axis --ts <file>             # TypeScript type definitions
axis --rust <file>           # Rust/axum server generation
axis --client-ts <file>      # TypeScript API client SDK
axis --client-rust <file>    # Rust API client SDK
axis --graphql <file>        # GraphQL schema
axis --deploy <file>         # deployment manifests (Dockerfile, Compose, K8s, .env)
axis --migrate <file>        # database migration plan with up/down SQL
axis --migrate-runner <file> # migration runner (Cargo.toml + main.rs)
axis --testgen <file>        # generated test suite (JSON)
axis --observability <file>  # observability schema + Prometheus config
axis --diff <old> <new>      # diff two axis files
axis --completions <file>    # completions at cursor position
axis --constrain             # export grammar for LLM guidance
axis --logit-masks           # export logit masks for constrained decoding
axis --project <dir>         # compile multi-file project
axis --watch <dir>           # watch and recompile on changes
axis --lsp                   # language server (stdio)
```

Reads from stdin if no file argument is provided.

Environment variables for `--serve`:

```
DATABASE_URL=postgres://user:pass@host:port/db   # PostgreSQL
DATABASE_URL=mysql://user:pass@host:port/db      # MySQL
DATABASE_URL=sqlite:path/to/db.sqlite            # SQLite
REDIS_URL=redis://127.0.0.1/                     # Redis (requires redis-port feature)
JWT_SECRET=your-secret                            # required for AUTH session
PORT=3000                                         # default 3000
```

Per-source overrides: `{SOURCE_NAME}_DATABASE_URL`, `{SOURCE_NAME}_REDIS_URL`.

## Multi-file projects

Split constructs across files — shapes in one, sources in another, flows in a third. The compiler merges them and verifies cross-references across the whole project.

```
axis --project ./my-api/
axis --watch ./my-api/
```

## Editor support

### VS Code

The `editors/vscode/` directory contains a VS Code extension with:

- Syntax highlighting (TextMate grammar)
- LSP integration (completions, go-to-definition, references, hover, rename, diagnostics)
- Workspace symbol search
- Watch file events

```
cd editors/vscode
npm install
npx tsc -p .
```

Then "Run Extension" from VS Code, or package with `npx vsce package`.

## Build

```
cargo build --release
cargo test
```

Optional features:

```
cargo build --features redis-port   # enable Redis SOURCE support
```

Requires Rust 1.85+.

## Projects

| Project | What | Scale |
|---------|------|-------|
| `projects/todolist/` | Basic CRUD proof | 20/20 integration tests |
| `projects/advanced/` | Full-feature proof (auth, tenants, guards, rules, funcs, match, try/recover, rate limiting, caching, surfaces) | 32/32 integration tests |
| `projects/helperbook/` | Production backend for a client-provider marketplace (auth, messaging, scheduling, commerce, reputation, admin) | 295 constructs, 171 endpoints, 16 services, 11 files |

```bash
axis --project projects/helperbook    # verify (0 errors)
axis --serve projects/helperbook      # serve (requires DATABASE_URL)
```

## Language design principles

- **Indentation-based** — 2-space indent, no braces, no semicolons
- **Prefix-only expressions** — operator comes first: `EQ a b`, `GT x y`, `MUL a b`
- **LL(1) grammar** — every parse decision is made from the current token, no backtracking
- **Constrained vocabulary** — small keyword set, making it easy for small LLMs to learn
- **Single-purpose constructs** — each construct does one thing; composition happens between them
