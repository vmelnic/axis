# Runtime

The `axis --serve` mode interprets the AST directly as a running HTTP server using axum and sqlx.

## Starting the Server

```bash
DATABASE_URL=postgres://user:pass@localhost/db axis --serve my-api/
```

The server compiles the project, verifies it, and starts listening on port 3000 (configurable via `--port` or `PORT` env var).

## Request Lifecycle

```
HTTP Request
  |
  |-- [1] Route Match         Surface -> Flow resolution. 404 if no match.
  |
  |-- [2] Rate Limit          Token bucket check. 429 if exceeded.
  |
  |-- [3] Parse Input         Body/Param/Header deserialization + type validation.
  |                           400 if invalid (structured error body).
  |
  |-- [4] Auth                Session/JWT/API key/Webhook signature verification.
  |                           401 if missing, 403 if insufficient.
  |
  |-- [5] Cache Lookup        Check response cache (GET flows only).
  |                           Return cached response if hit.
  |
  |-- [6] Tenant Scope        Inject tenant filter into query context.
  |
  |-- [7] Execute Steps       LET, FETCH, QUERY, CALL, MATCH, RULE, GUARD, etc.
  |                           DB transaction opened on first mutation.
  |
  |-- [8] Mutate              INSERT/UPDATE/DELETE within transaction.
  |                           Effects queued within same transaction.
  |                           Transaction committed.
  |
  |-- [9] Effects             Broadcast to stream subscribers.
  |
  |-- [10] Respond            Surface filter -> JSON serialization -> HTTP response.
  |
  |-- [11] Cache Store        Store response in cache if applicable.
```

## Authentication

### JWT (session / bearer)

The runtime decodes JWT tokens from the `Authorization: Bearer <token>` header using HS256 algorithm. The secret is loaded from `JWT_SECRET` environment variable.

Claims are available as `auth.*` bindings in the flow:

```axis
AUTH session
-- auth.user_id, auth.email, auth.role, etc.
```

### API Key

Reads the `x-api-key` header. The key ID is available as `auth.key_id`.

### Webhook Signature

Verifies HMAC-SHA256 signature from the `x-signature` or `x-hub-signature-256` header against the request body:

```axis
AUTH WEBHOOK SIGNATURE stripe_webhook_secret HMAC sha256
```

## Rate Limiting

Sliding window implementation using in-memory timestamps.

Supported scopes:
- `per_user` -- keyed by `sub` or `user_id` from JWT claims
- `per_ip` -- keyed by source IP address
- `per_key` -- keyed by API key
- `global` -- single global counter

Returns 429 Too Many Requests when exceeded.

## Response Caching

For GET flows with CACHE declarations:

1. Cache key = flow name + path + query params + VARY fields.
2. On cache hit, return the stored response immediately.
3. On cache miss, execute the flow and store the response with TTL.

## Tenant Scoping

When a flow declares `SCOPE TENANT auth.user_id`:

1. The runtime resolves the tenant value from the auth context.
2. Looks up the realm's TENANT field name.
3. Automatically injects `WHERE <tenant_field> = <tenant_value>` into every SQL query on tenanted sources.

`SCOPE TENANT ANY` skips the injection (admin access).

## Database Operations

The runtime uses sqlx with connection pooling:

- **PostgreSQL**: PgPool, `$1`/`$2` placeholders, RETURNING support
- **MySQL**: MySqlPool, `?` placeholders, no RETURNING (uses client-side UUID + SELECT back for INSERT)
- **SQLite**: SqlitePool, `?` placeholders, RETURNING support

Pool sizes: 10 max connections for Postgres/MySQL, 5 for SQLite.

Type mappings vary by dialect -- see [Multi-Database Support](multi-database.md).

## Stream Connections

### WebSocket

1. Client connects to the WebSocket path.
2. Auth is checked before upgrade.
3. Connection subscribes to the stream's broadcast channel.
4. Server filters events by name and sends matching ones as JSON.
5. Client messages are dispatched to RECEIVE handlers.

### SSE

1. Client connects to the SSE endpoint.
2. Auth is checked before streaming.
3. Events are sent in SSE protocol format: `event: <name>\ndata: <json>\n\n`.

## Effects

Effects are broadcast via tokio broadcast channels. All connected stream subscribers receive matching events.

Effect types at runtime:
- `email` -- resolved template + recipient
- `push_notification` -- event name + data
- `ASYNC` -- task name + data
- `webhook` -- HTTP POST to URL (not executed in interpreter mode -- broadcast only)

## Template Rendering

Templates are loaded from the templates directory (`--templates` or `AXIS_TEMPLATES_DIR`).

The `RENDER` expression performs `{{key}}` variable substitution in template strings.

## Internationalization (i18n)

Locale files (JSON) are loaded from the locales directory (`--locales` or `AXIS_LOCALES_DIR`).

The `T` (translate) expression resolves locale from the `_locale` binding, falls back to `DEFAULT_LOCALE` (default: `en`), and supports dot-notation for nested keys with `{{var}}` substitution.

## File Storage

For STORAGE constructs with `BACKEND local`:

- Files are written to `{bucket}/{prefix}/{uuid_filename}`.
- Public storages are served via static file handler at `/files/{bucket}/{prefix}/`.
- S3 backend is parsed but returns "not implemented" at runtime.

## Service Calls

External SERVICE calls are made via HTTP (reqwest):

- Retry logic with exponential or linear backoff.
- 4xx errors are not retried; 5xx errors are.
- Timeout per METHOD declaration.

## Adapters

The runtime supports hot-swappable service adapters loaded from the adapters directory. Adapter configuration is reloaded every 2 seconds.

Adapters can:
- Return static data
- Transform and forward HTTP requests
- Support URL variable substitution from config

## Metrics

The runtime exposes a `/metrics` endpoint in Prometheus format:

```
axis_request_count{flow="get_user",method="GET",status="200"} 42
axis_request_errors{flow="get_user",method="GET"} 0
axis_request_latency_sum{flow="get_user"} 1.234
axis_request_latency_count{flow="get_user"} 42
```

Metrics are recorded after each request with flow name, HTTP method, and status code.

## Error Response Format

All errors return JSON:

```json
{
  "error": "message describing what went wrong"
}
```

HTTP status codes come from the flow definition (OR clauses, GUARD codes, etc.). Internal errors return 500 and log to stderr.

## Health Check

The runtime serves standard health endpoints from SURFACE route mappings. Configure explicit health routes in your flows if needed.
