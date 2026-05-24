# SOURCE

Maps a shape to a physical datastore. Every FETCH, QUERY, INSERT, UPDATE, and DELETE operation targets a source, not a shape directly.

## Syntax

```axis
SOURCE <name> <type>
  SHAPE <ShapeName>
  [INDEX <field1> [field2...] [suffix]]
  ...
  [TTL <seconds>]
```

## Source Types

| Type | Runtime Support | Operations |
|------|----------------|------------|
| `POSTGRES` | Full (sqlx PgPool) | FETCH, QUERY, INSERT, UPDATE, DELETE |
| `MYSQL` | Full (sqlx MySqlPool) | FETCH, QUERY, INSERT, UPDATE, DELETE |
| `SQLITE` | Full (sqlx SqlitePool) | FETCH, QUERY, INSERT, UPDATE, DELETE |
| `REDIS` | Via adapter (feature-gated: `redis-port`) | FETCH, INSERT (SET), DELETE |
| `ELASTICSEARCH` | Parsed and verified, no runtime port | QUERY only |
| `DYNAMODB` | Parsed and verified, no runtime port | FETCH, QUERY, INSERT, UPDATE, DELETE |

## Examples

```axis
SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE
  INDEX created_at DESC

SOURCE bookings MYSQL
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out
  INDEX status

SOURCE settings SQLITE
  SHAPE UserSetting
  INDEX user_id UNIQUE

SOURCE listing_cache REDIS
  SHAPE ListingCache
  TTL 3600

SOURCE listing_search ELASTICSEARCH
  SHAPE ListingSearch
  INDEX location GEO
  INDEX title TEXT
  INDEX amenities KEYWORD
```

## INDEX Clause

Indexes declare which query patterns are supported. The compiler verifies every QUERY and FETCH can be served by a declared index.

### Index Rules

- Fields listed left-to-right match the index column order.
- The compiler checks that the leading column of at least one index appears in the FILTER clause of every QUERY/FETCH.
- If no index covers a query, the compiler rejects with a suggested index.

### Index Suffixes

| Suffix | Description |
|--------|-------------|
| `ASC` | Ascending order (default) |
| `DESC` | Descending order |
| `UNIQUE` | Unique index constraint |
| `GEO` | Geo-spatial index (Elasticsearch) |
| `TEXT` | Full-text search index (Elasticsearch) |
| `KEYWORD` | Exact-match index on array fields (Elasticsearch) |

Each field in the index can have its own suffix:

```axis
INDEX user_id created_at DESC
INDEX email UNIQUE
INDEX location GEO
```

## TTL

For Redis sources, `TTL <seconds>` sets the default expiration time for keys.

## Connection Configuration

Each source connects via environment variables:

| Pattern | Example | Description |
|---------|---------|-------------|
| `DATABASE_URL` | `postgres://user:pass@host/db` | Default for all SQL sources |
| `{SOURCE_NAME}_DATABASE_URL` | `BOOKINGS_DATABASE_URL` | Per-source override |
| `REDIS_URL` | `redis://127.0.0.1/` | Default for Redis sources |
| `{SOURCE_NAME}_REDIS_URL` | `CACHE_REDIS_URL` | Per-source Redis override |

Connection pool sizes: 10 max for Postgres/MySQL, 5 for SQLite.

## Compiler Checks

- Duplicate source names are rejected.
- The SHAPE referenced must exist.
- Index fields must exist in the referenced shape.
- Operations on sources are validated against the source type (e.g., QUERY on Redis is rejected).
- Every QUERY/FETCH in a flow must be covered by a declared index.

## Multi-Database Support

See [Multi-Database Support](../multi-database.md) for details on how Axis handles dialect differences across Postgres, MySQL, and SQLite.
