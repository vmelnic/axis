# Getting Started

## Installation

Axis requires Rust 1.85 or later.

```bash
git clone <repo-url>
cd axis
cargo build --release
```

The binary is at `target/release/axis`. Add it to your PATH or use it directly.

Optional features:

```bash
cargo build --release --features redis-port   # enable Redis SOURCE support
```

## Your First Axis Program

Create a file called `hello.axis`:

```axis
SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED
  created_at TIMESTAMP AUTO

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE

REALM api
  CAPABILITY read users
  CAPABILITY write users

FLOW list_users GET /users
  REALM api
  AUTH none
  LET all_users
    QUERY users
  RETURN 200 all_users

FLOW get_user GET /users/:id
  REALM api
  AUTH none
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user

FLOW create_user POST /users
  REALM api
  AUTH none
  BODY NewUser
    email STRING 255 REQUIRED
    name STRING 100 REQUIRED
  INSERT users
    email body.email
    name body.name
  AS user
  RETURN 201 user
```

## Check Your Program

Parse, link, and verify without running:

```bash
axis hello.axis
```

Output on success:

```
OK: 1 shapes, 1 sources, 1 realms, 3 flows, 0 streams -- verified
```

## Generate SQL

```bash
axis --sql hello.axis
```

Produces the DDL for your database tables.

## Serve Your API

Start a live HTTP server that interprets the AST directly:

```bash
DATABASE_URL=postgres://user:pass@localhost/mydb axis --serve .
```

The server starts on port 3000 by default. Override with `--port` or `PORT` env var.

```bash
PORT=8080 axis --serve .
```

### Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `DATABASE_URL` | Database connection string | `postgres://user:pass@host/db` |
| `JWT_SECRET` | JWT signing secret (required for `AUTH session`/`bearer`) | `your-secret-key` |
| `PORT` | Server port (default: 3000) | `8080` |
| `REDIS_URL` | Redis connection (requires `redis-port` feature) | `redis://127.0.0.1/` |

Per-source overrides use the source name as prefix: `USERS_DATABASE_URL`, `CACHE_REDIS_URL`.

## Multi-File Projects

For larger projects, split constructs across files in a directory:

```
my-api/
  src/
    models.axis      # shapes
    sources.axis     # sources, realms
    endpoints.axis   # flows
```

Compile and verify the entire project:

```bash
axis --project my-api/
```

Serve the project:

```bash
axis --serve my-api/
```

The compiler merges all `.axis` files and verifies cross-references across the whole project.

## Watch Mode

Recompile automatically on file changes:

```bash
axis --watch my-api/
```

## Editor Support

The `editors/vscode/` directory contains a VS Code extension with syntax highlighting and LSP integration. See [Editor Support](tooling.md) for setup instructions.

## Next Steps

- [Language Overview](language-overview.md) -- understand the syntax rules
- [Type System](type-system.md) -- learn about types and modifiers
- [FLOW](constructs/flow.md) -- the core construct for API endpoints
- [CLI Reference](cli-reference.md) -- all available commands
