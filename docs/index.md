# Axis Language Documentation

Axis is a programming language, compiler, and runtime for backend HTTP APIs. It uses 12 top-level constructs to fully specify a production backend: data models, databases, endpoints, auth, permissions, distributed transactions, API surfaces, policies, migrations, real-time streams, reusable functions, and file storage.

The `axis` binary is self-contained -- it parses `.axis` files, generates artifacts, and serves a full HTTP API directly from the AST.

## Table of Contents

### Getting Started

- [Getting Started](getting-started.md) -- Installation, first program, running the server

### Language Reference

- [Language Overview](language-overview.md) -- Design principles, lexical structure, syntax rules
- [Type System](type-system.md) -- Primitive types, compound types, modifiers, type rules
- [Expressions](expressions.md) -- Arithmetic, comparison, string, date/time, aggregate, control flow

### Constructs

- [SHAPE](constructs/shape.md) -- Data model with typed fields and constraints
- [SOURCE](constructs/source.md) -- Database binding with indexes
- [REALM](constructs/realm.md) -- Permission scope with capabilities and tenant isolation
- [FLOW](constructs/flow.md) -- API endpoint with auth, guards, data ops, and response
- [SAGA](constructs/saga.md) -- Distributed transaction with compensating steps
- [SURFACE](constructs/surface.md) -- API versioning and routing layer
- [POLICY](constructs/policy.md) -- Compile-time invariants enforced across flows
- [SERVICE](constructs/service.md) -- External service integration
- [MIGRATE](constructs/migrate.md) -- Schema migration with up/down operations
- [STREAM](constructs/stream.md) -- Real-time events over WebSocket or SSE
- [FUNC](constructs/func.md) -- Reusable pure function with typed inputs/output
- [STORAGE](constructs/storage.md) -- File storage backend with upload support

### Operations & Runtime

- [Flow Operations](flow-operations.md) -- AUTH, BODY, PARAM, HEADER, RULE, GUARD, LET, FETCH, QUERY, INSERT, UPDATE, DELETE, CALL, EFFECT, MATCH, EACH, TRY/RECOVER, SET, UPLOAD, RETURN
- [Runtime](runtime.md) -- Serve mode, request lifecycle, error responses, observability
- [Multi-Database Support](multi-database.md) -- PostgreSQL, MySQL, SQLite, Redis

### Tooling

- [CLI Reference](cli-reference.md) -- All commands, flags, and options
- [Code Generation](code-generation.md) -- SQL, Rust, TypeScript, OpenAPI, GraphQL, clients, deployment
- [Editor Support](tooling.md) -- LSP, VS Code extension, formatter, constrained decoding

### Examples

- [Examples](examples.md) -- Annotated example programs
