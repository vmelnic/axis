# Code Generation

Axis generates multiple output formats from the same `.axis` source. All generators perform direct AST-to-output transformation.

## SQL DDL (`--sql`)

Generates CREATE TABLE and CREATE INDEX statements. Dialect-aware based on SOURCE type.

```bash
axis --sql app.axis
```

Features:
- Table creation from SHAPE + SOURCE definitions
- Primary key, unique, and foreign key constraints
- Index creation matching SOURCE INDEX declarations
- Dialect-specific type mappings (see [Multi-Database Support](multi-database.md))
- Per-source DDL (each source gets its own dialect-appropriate schema)

## Rust Server (`--rust`)

Generates a complete, compilable Rust/axum server project.

```bash
axis --rust app.axis
```

Output:
- `Cargo.toml` with sqlx, axum, and other dependencies (only includes features for backends used)
- `src/main.rs` with routes, handlers, database pools, and middleware
- `schema.sql` with database schema

The generated server handles:
- Multi-dialect database access (per-source pool types)
- Dialect-correct SQL placeholders and RETURNING handling
- JWT auth, rate limiting, response caching
- Request validation and error handling

## TypeScript Types (`--ts`)

Generates TypeScript type definitions and runtime support code.

```bash
axis --ts app.axis
```

Output sections:
- `types.ts` -- interface definitions for all shapes
- `validators.ts` -- runtime validation functions for request bodies
- `queries.ts` -- typed database query helpers
- `handlers.ts` -- route handler type signatures
- `router.ts` -- route registration code

## OpenAPI Specification (`--openapi`)

Generates an OpenAPI 3.1 JSON specification.

```bash
axis --openapi app.axis
axis --project --openapi my-api/
```

Includes:
- All FLOW and SAGA endpoints with paths, methods, and descriptions
- Request body schemas from BODY declarations
- Response schemas from RETURN statements
- Query parameter definitions from PARAM declarations
- Auth requirements from AUTH declarations
- SURFACE route mappings applied as path prefixes

## GraphQL Schema (`--graphql`)

Generates a GraphQL schema from shapes and flows.

```bash
axis --graphql app.axis
```

Maps shapes to GraphQL types and flows to queries/mutations based on HTTP method.

## Client SDKs

### TypeScript Client (`--client-ts`)

```bash
axis --client-ts app.axis
```

Generates a typed TypeScript API client with methods for each flow.

### Rust Client (`--client-rust`)

```bash
axis --client-rust app.axis
```

Generates a typed Rust API client with methods for each flow.

## Deployment Manifests (`--deploy`)

Generates deployment configuration files.

```bash
axis --deploy app.axis
```

Output:
- `Dockerfile` -- multi-stage build for the axis binary
- `docker-compose.yml` -- local development setup with database services
- `k8s.yaml` -- Kubernetes deployment, service, and configmap
- `.env` -- environment variable template

## Migration SQL (`--migrate`)

Generates migration files from MIGRATE constructs.

```bash
axis --migrate app.axis
```

Produces numbered migration files with:
- Up SQL (ALTER TABLE, CREATE INDEX, data backfill)
- Down SQL (reverse operations)
- Migration metadata as JSON

## Migration Runner (`--migrate-runner`)

Generates a standalone Rust project that applies migrations.

```bash
axis --migrate-runner app.axis
```

Output:
- `Cargo.toml` with sqlx dependency
- `src/main.rs` with migration execution logic

## Test Suite (`--testgen`)

Generates test cases from flow definitions.

```bash
axis --testgen app.axis
```

Outputs a JSON test suite with test cases covering:
- Happy path for each flow
- Auth failure cases
- Validation failure cases (guard violations)
- Edge cases from PARAM constraints

## Observability Config (`--observability`)

Generates observability schema and monitoring configuration.

```bash
axis --observability app.axis
```

Outputs:
- Metrics definitions (counters, histograms per flow/source/service)
- Trace schema (span definitions per flow step)
- Log field definitions
- Prometheus scrape configuration

## Execution Plan (`--plan`)

Shows the optimized execution plan with warnings.

```bash
axis --plan app.axis
```

Displays:
- Execution groups per flow (sequential vs parallel steps)
- Transaction boundaries (which mutations are wrapped together)
- Outbox effects per transaction
- Saga step transactions and compensation chains

## Cross-Reference Links (`--link`)

Shows the content-addressed manifest with cross-references.

```bash
axis --link app.axis
```

Displays:
- Manifest hash
- Each construct with its content hash
- Dependency links between constructs

## Structural Diff (`--diff`)

Compares two axis files and shows structural differences.

```bash
axis --diff old.axis new.axis
```

Reports added, removed, and modified constructs with migration suggestions.

## Constrained Decoding (`--constrain`, `--logit-masks`)

Exports grammar information for LLM constrained decoding.

```bash
axis --constrain        # grammar state machine as JSON
axis --logit-masks      # vocabulary + logit masks per state
```

The LL(1) grammar enables constrained decoding: at each token position, the set of valid next tokens is deterministic. These exports provide the data needed to mask LLM logits during generation.
