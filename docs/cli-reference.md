# CLI Reference

## Usage

```
axis [MODE] [OPTIONS] <file|dir>
```

Reads from stdin if no file argument is provided.

## Modes

### Check (default)

Parse, link, and verify a single `.axis` file.

```bash
axis app.axis
```

Output: `OK: N shapes, N sources, N realms, N flows, N streams -- verified`

### --project

Compile a multi-file project directory.

```bash
axis --project my-api/
```

Merges all `.axis` files in the directory (or `<dir>/src/` if it exists) and verifies cross-references.

### --serve

Start an HTTP server that interprets the AST at runtime.

```bash
axis --serve my-api/
axis --serve --port 8080 my-api/
```

Requires `DATABASE_URL` environment variable. See [Runtime](runtime.md) for details.

### --sql

Generate SQL DDL.

```bash
axis --sql app.axis
```

Produces CREATE TABLE, CREATE INDEX, and constraint statements. Dialect-aware based on the SOURCE type.

### --fmt

Format source code.

```bash
axis --fmt app.axis
```

Pretty-prints the AST with consistent indentation and spacing.

### --routes

List all routes.

```bash
axis --routes app.axis
```

### --openapi

Generate OpenAPI 3.0 specification.

```bash
axis --openapi app.axis
```

Outputs JSON. Includes all flows, request/response schemas, and auth requirements.

### --ts

Generate TypeScript type definitions.

```bash
axis --ts app.axis
```

Produces:
- `types.ts` -- shape type definitions
- `validators.ts` -- runtime validation functions
- `queries.ts` -- database query types
- `handlers.ts` -- route handler types
- `router.ts` -- route registration

### --rust

Generate a complete Rust/axum server project.

```bash
axis --rust app.axis
```

Produces:
- `Cargo.toml` -- project manifest with dependencies
- `src/main.rs` -- axum server with all routes
- `schema.sql` -- database schema

The generated server is multi-dialect: it includes per-source pool fields and dialect-correct SQL.

### --graphql

Generate GraphQL schema.

```bash
axis --graphql app.axis
```

### --client-ts

Generate TypeScript API client SDK.

```bash
axis --client-ts app.axis
```

### --client-rust

Generate Rust API client SDK.

```bash
axis --client-rust app.axis
```

### --deploy

Generate deployment manifests.

```bash
axis --deploy app.axis
```

Produces:
- `Dockerfile` -- multi-stage build
- `docker-compose.yml` -- local development setup
- `k8s.yaml` -- Kubernetes deployment, service, config
- `.env` -- environment variable template

### --migrate

Generate migration SQL with up/down statements.

```bash
axis --migrate app.axis
```

Produces numbered migration files with SQL for each MIGRATE construct.

### --migrate-runner

Generate a standalone Rust migration runner project.

```bash
axis --migrate-runner app.axis
```

Produces a `Cargo.toml` and `src/main.rs` that apply migrations.

### --testgen

Generate test suite.

```bash
axis --testgen app.axis
```

Outputs a JSON test suite with test cases for all flows, sagas, and streams.

### --observability

Generate observability schema and Prometheus configuration.

```bash
axis --observability app.axis
```

Outputs metrics, traces, and log definitions plus a Prometheus scrape config.

### --emit

Emit all codegen output (SQL, routes, migrations, sagas, surfaces, streams).

```bash
axis --emit app.axis
```

### --plan

Show execution plan with warnings.

```bash
axis --plan app.axis
```

Displays execution groups, parallelism opportunities, and transaction boundaries for each flow.

### --link

Show cross-reference links and content-addressed manifest.

```bash
axis --link app.axis
```

### --diff

Structural diff between two axis files.

```bash
axis --diff old.axis new.axis
```

### --completions

Incremental validation and completion suggestions.

```bash
axis --completions app.axis
```

Reports parser state, valid next tokens, and any errors found during incremental parsing.

### --constrain

Export grammar state machine for LLM constrained decoding.

```bash
axis --constrain
```

Outputs JSON describing the grammar states and valid transitions.

### --logit-masks

Export logit masks for constrained decoding.

```bash
axis --logit-masks
```

Outputs a JSON object with vocabulary and mask arrays for each grammar state.

### --watch

Watch directory and recompile on changes.

```bash
axis --watch my-api/
```

Monitors `.axis` files and re-runs compilation + codegen on modifications.

### --lsp

Start the language server (stdio protocol).

```bash
axis --lsp
```

Provides completions, go-to-definition, references, hover, rename, diagnostics, and workspace symbol search.

## Options

| Option | Description | Default |
|--------|-------------|---------|
| `--port <N>` | Server port for `--serve` | 3000 (env: `PORT`) |
| `--src <dir>` | Source directory for `.axis` files | `<dir>/src` or `<dir>` |
| `--adapters <dir>` | Adapters directory | `<dir>/adapters` (env: `AXIS_ADAPTERS_DIR`) |
| `--templates <dir>` | Templates directory | `<dir>/templates` (env: `AXIS_TEMPLATES_DIR`) |
| `--locales <dir>` | Locales directory | `<dir>/locales` (env: `AXIS_LOCALES_DIR`) |
| `--db <url>` | Database URL | env: `DATABASE_URL` |
| `--jwt-secret <s>` | JWT signing secret | env: `JWT_SECRET` |
| `-h`, `--help` | Show help | |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | Database connection string (`postgres://`, `mysql://`, `sqlite:`) |
| `{SOURCE}_DATABASE_URL` | Per-source database URL override |
| `JWT_SECRET` | JWT signing secret |
| `PORT` | Server port |
| `REDIS_URL` | Redis connection URL (requires `redis-port` feature) |
| `{SOURCE}_REDIS_URL` | Per-source Redis URL override |
| `AXIS_SRC_DIR` | Source directory override |
| `AXIS_ADAPTERS_DIR` | Adapters directory override |
| `AXIS_TEMPLATES_DIR` | Templates directory override |
| `AXIS_LOCALES_DIR` | Locales directory override |
| `DEFAULT_LOCALE` | Default locale code (default: `en`) |
