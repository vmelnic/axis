# Multi-Database Support

Axis supports PostgreSQL, MySQL, and SQLite as SQL backends, with Redis available via a feature-gated adapter. Elasticsearch and DynamoDB are parsed and verified but have no runtime implementation yet.

## Source Type Support

| Source Type | Runtime | DDL Codegen | Migration | Adapter |
|-------------|---------|-------------|-----------|---------|
| `POSTGRES` | sqlx PgPool | Full | Full | Native |
| `MYSQL` | sqlx MySqlPool | Full | Full | Native |
| `SQLITE` | sqlx SqlitePool | Full | Full | Native |
| `REDIS` | redis crate | -- | -- | SourceAdapter trait (`redis-port` feature) |
| `ELASTICSEARCH` | -- | -- | -- | Parsed only |
| `DYNAMODB` | -- | -- | -- | Parsed only |

## SQL Dialect Differences

### Placeholder Style

| Dialect | Style | Example |
|---------|-------|---------|
| Postgres | Positional `$N` | `WHERE id = $1 AND status = $2` |
| MySQL | Positional `?` | `WHERE id = ? AND status = ?` |
| SQLite | Positional `?` | `WHERE id = ? AND status = ?` |

### RETURNING Clause

| Dialect | Support | INSERT Strategy |
|---------|---------|-----------------|
| Postgres | `RETURNING *` | Direct return |
| MySQL | Not supported | Client-side UUID + SELECT back |
| SQLite | `RETURNING *` | Direct return |

MySQL INSERT generates a UUID on the client, inserts with the pre-generated UUID, then SELECTs the row back.

### Type Mappings

| Axis Type | Postgres | MySQL | SQLite |
|-----------|----------|-------|--------|
| `UUID` | `UUID` | `CHAR(36)` | `TEXT` |
| `STRING N` | `VARCHAR(N)` | `VARCHAR(N)` | `TEXT` |
| `TEXT` | `TEXT` | `TEXT` | `TEXT` |
| `INT` | `BIGINT` | `BIGINT` | `INTEGER` |
| `DECIMAL` | `NUMERIC(P,S)` | `DECIMAL(P,S)` | `REAL` |
| `BOOL` | `BOOLEAN` | `BOOLEAN` | `INTEGER` |
| `DATE` | `DATE` | `DATE` | `TEXT` |
| `TIMESTAMP` | `TIMESTAMPTZ` | `DATETIME` | `TEXT` |
| `JSON` | `JSONB` | `JSON` | `TEXT` |
| `BLOB` | `BYTEA` | `LONGBLOB` | `BLOB` |

### Index Differences

| Feature | Postgres | MySQL | SQLite |
|---------|----------|-------|--------|
| GIN/GiST indexes | Native | Falls back to regular | Falls back to regular |
| Partial indexes | Supported | Not generated | Not generated |
| Expression indexes | Supported | Not generated | Not generated |

### NOW() Expression

| Dialect | SQL |
|---------|-----|
| Postgres | `NOW()` |
| MySQL | `NOW()` |
| SQLite | `datetime('now')` |

## Connection Configuration

### Single Database

Set `DATABASE_URL`:

```bash
DATABASE_URL=postgres://user:pass@localhost:5432/mydb axis --serve .
DATABASE_URL=mysql://user:pass@localhost:3306/mydb axis --serve .
DATABASE_URL=sqlite:path/to/db.sqlite axis --serve .
```

### Multiple Databases

Use per-source environment variables:

```bash
USERS_DATABASE_URL=postgres://user:pass@host1/users
ORDERS_DATABASE_URL=mysql://user:pass@host2/orders
CACHE_DATABASE_URL=sqlite:cache.db
axis --serve .
```

The pattern is `{SOURCE_NAME}_DATABASE_URL` (uppercase source name).

### Redis

Requires the `redis-port` feature:

```bash
cargo build --features redis-port
REDIS_URL=redis://127.0.0.1/ axis --serve .
```

Per-source: `{SOURCE_NAME}_REDIS_URL`.

## Connection Pools

| Backend | Max Connections |
|---------|----------------|
| Postgres | 10 |
| MySQL | 10 |
| SQLite | 5 |

## Redis Adapter

The Redis adapter implements the `SourceAdapter` trait with:

- `FETCH` -- Redis GET by key
- `INSERT` -- Redis SET with optional TTL
- `DELETE` -- Redis DEL

QUERY and UPDATE are not supported on Redis sources. The compiler rejects these operations.

## Generated Server (--rust)

The generated Rust server is multi-dialect aware:

- Per-source pool fields with correct types (`PgPool`, `MySqlPool`, `SqlitePool`)
- Dialect-correct placeholder generation
- RETURNING handling per dialect
- NOW() expression per dialect
- Only includes sqlx features for backends actually used in the program
