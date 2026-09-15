# Flow Operations

This reference covers all operations available within a [FLOW](constructs/flow.md) or [SAGA](constructs/saga.md) step.

## AUTH

Declares authentication and authorization requirements.

```axis
AUTH none
AUTH session
AUTH bearer
AUTH api_key
AUTH role admin
AUTH role IN admin moderator
AUTH WEBHOOK SIGNATURE secret_name HMAC sha256
```

| Type | Description |
|------|-------------|
| `none` | No authentication. Must be explicit. |
| `session` | Cookie-based session / JWT. `auth.user_id`, `auth.email`, etc. available. |
| `bearer` | JWT bearer token in Authorization header. Claims available as `auth.*`. |
| `api_key` | API key in `x-api-key` header. `auth.key_id` available. |
| `role <name>` | Requires the specified role. Shorthand for session + role check. |
| `role IN <names...>` | Requires any of the listed roles. |
| `WEBHOOK SIGNATURE <secret> HMAC <algorithm>` | HMAC signature verification (e.g., Stripe webhooks). Secret resolved from vault. |

The runtime resolves auth before any flow operation runs. Auth failure returns 401 (unauthenticated) or 403 (unauthorized).

## BODY

Declares the request body shape, parsed from JSON.

```axis
BODY BookingCreate
  listing_id UUID REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  guest_count INT MIN 1 MAX 16 DEFAULT 1
  note MAYBE TEXT
```

Type validation is automatic: if `guest_count` arrives as `"abc"`, the runtime returns 400 with a structured error.

### MULTIPART

For file uploads, use MULTIPART:

```axis
BODY MULTIPART AvatarUpload
  file BLOB REQUIRED
  description MAYBE TEXT
```

## PARAM

Declares query string parameters.

```axis
PARAM page INT DEFAULT 1 MIN 1
PARAM page_size INT DEFAULT 20 MIN 1 MAX 100
PARAM status MAYBE ENUM pending confirmed cancelled
PARAM sort_by ENUM created_at price DEFAULT created_at
```

Accessed as `query.<name>` in expressions.

## HEADER

Declares request headers.

```axis
HEADER idempotency_key UUID REQUIRED
HEADER accept_language STRING DEFAULT "en"
```

Accessed as `header.<name>` in expressions.

## IDEMPOTENCY

Makes a mutating flow exactly-once from the caller's perspective.

```axis
IDEMPOTENCY header.idempotency_key SCOPE auth.user_id TTL 86400
```

- `key` and `scope` must resolve to scalar values. Keys are limited to 255 bytes and scopes to 512 bytes.
- The first request reserves `(flow, scope, key)` inside the same transaction as all flow SQL.
- An identical retry returns the previously committed status, JSON body, and response headers. `Idempotency-Replayed` is `false` on the first response and `true` on replay.
- Reusing the key with a different path, query, or body returns `409`.
- Failed and timed-out flows roll back both application writes and the reservation, so a corrected retry can execute.
- `TTL` is in seconds. Expired reservations are reclaimed transactionally and stale rows are cleaned periodically.

IDEMPOTENCY is invalid on GET or a flow without a database mutation. Every accessed source must be PostgreSQL, MySQL, or SQLite, use the same dialect, and resolve to the same database URL. `TRY` and `UPLOAD` are rejected. A direct `CALL` is accepted only for a `PURE` method or for a method with provider `IDEMPOTENCY` whose named argument is the exact flow key; use `EFFECT` for all other external work.

## LIMIT

Rate limiting.

```axis
LIMIT 10 per_minute per_user
LIMIT 100 per_hour per_user
LIMIT 1000 per_hour global
```

| Unit | Duration |
|------|----------|
| `per_second` | 1 second window |
| `per_minute` | 1 minute window |
| `per_hour` | 1 hour window |
| `per_day` | 1 day window |

| Scope | Key |
|-------|-----|
| `per_user` | `auth.user_id` or `auth.sub` |
| `per_ip` | Source IP address |
| `per_key` | API key |
| `global` | System-wide |

Multiple LIMIT declarations stack -- all must pass. Runtime uses a sliding window implementation. Returns 429 Too Many Requests on violation.

## CACHE

Response caching. Only allowed on GET flows.

### Flow-Level Cache

```axis
CACHE 60 VARY path.id
CACHE 300 VARY path.id auth.user_id
```

`CACHE <ttl_seconds> [VARY <field1> [field2...]]` caches the entire response. The cache key includes the flow name, path, query parameters, and VARY fields.

### Inline Query Cache

```axis
LET popular
  CACHE 300
    QUERY listings
      FILTER status EQ active
      SORT booking_count DESC
      PAGE_SIZE 10
```

Wraps a specific query with a cache. Cached by content hash of the query.

## SCOPE

Tenant scoping.

```axis
SCOPE TENANT auth.user_id
SCOPE TENANT ANY
```

When declared, the runtime injects `WHERE <tenant_field> = <value>` into every QUERY and FETCH on tenanted sources within the flow.

`SCOPE TENANT ANY` bypasses tenant filtering. Requires `CAPABILITY admin` in the realm.

## TIMEOUT

Sets a timeout for the entire flow execution.

```axis
TIMEOUT 30s
```

## RULE

Named authorization checks reusable across flows.

```axis
RULE user_may_book
  REQUIRE auth.email_verified EQ TRUE
  REQUIRE auth.account_status EQ active
  REQUIRE auth.ban_status NEQ banned
```

Each `REQUIRE` clause is a boolean assertion. All must be true. Failure returns 403.

Rules can reference bindings declared earlier in the flow:

```axis
RULE is_owner
  REQUIRE auth.user_id EQ listing.host_id
```

## GUARD

Inline validation. Guards short-circuit with an error if the condition is false.

```axis
GUARD valid_dates 400 "check_out must be after check_in"
  GT body.check_out body.check_in

GUARD no_overlap 409 "dates are unavailable"
  EMPTY conflicts

GUARD capacity 400 "exceeds max guests"
  LTE body.guest_count listing.max_guests
```

Syntax: `GUARD <name> <http_code> [<message>]` followed by an indented boolean expression.

## LET

Bind a name to the result of a single operation. Immutable.

```axis
LET nights
  DAYS_BETWEEN body.check_in body.check_out

LET total
  MUL listing.price_per_night nights

LET has_discount
  AND
    listing.weekly_discount
    GTE nights 7
```

Bindings are available to all subsequent operations. Forward references are compile errors.

## SET

Update an existing binding with a new value.

```axis
SET total
  ADD total service_fee
```

Unlike LET, SET modifies a previously declared binding.

## FETCH

Single-row lookup. Returns exactly one row or fires the OR clause.

```axis
LET booking
  FETCH bookings
    FILTER id EQ path.id
  OR 404

LET listing
  FETCH listings
    FILTER id EQ booking.listing_id
    WITH reviews
  OR 500 "listing not found"
```

### FILTER

`FILTER <field> <op> <value>` -- where conditions. Multiple FILTERs are AND-ed.

Filter operators: `EQ`, `NEQ`, `GT`, `GTE`, `LT`, `LTE`, `IN`, `BETWEEN`, `LIKE`, `STARTS_WITH`, `CONTAINS`.

### WITH

`WITH <relation>` -- eager-load a related entity (resolved by foreign key).

### OR

`OR <code> [<message>]` is required on FETCH. Fires when zero rows match.

The OR clause can optionally include an error shape for structured error responses:

```axis
LET user
  FETCH users
    FILTER id EQ path.id
  OR 404 "user not found" UserNotFoundError
    field "id"
    value path.id
```

## QUERY

Multi-row query with filtering, sorting, and pagination.

```axis
LET recent_bookings
  QUERY bookings
    FILTER user_id EQ auth.user_id
    FILTER status IN confirmed completed
    FILTER check_in GTE NOW_MINUS 90 days
    SORT created_at DESC
    CURSOR query.cursor
    PAGE_SIZE query.page_size
```

### FILTER / SORT

Same operators as FETCH. `SORT <field> <ASC|DESC>` for ordering. Multiple SORT clauses for compound ordering.

### CURSOR / PAGE_SIZE

Keyset pagination. CURSOR is an opaque cursor value (omit for first page). PAGE_SIZE sets max rows returned.

When CURSOR and PAGE_SIZE are used, QUERY returns a pagination wrapper:

```json
{
  "items": [...],
  "total": 142,
  "next_cursor": "eyJpZCI6...",
  "has_more": true
}
```

Access via: `result.items`, `result.total`, `result.next_cursor`, `result.has_more`.

### No OR Clause

QUERY always succeeds (returns empty list on no matches). No OR clause needed.

## INSERT

Insert a new row into a source.

```axis
INSERT bookings
  user_id auth.user_id
  listing_id body.listing_id
  check_in body.check_in
  status pending
  total_price total
AS booking
```

`AS <name>` binds the inserted row (including AUTO fields like `id`, `created_at`).

The compiler verifies:
- All REQUIRED fields are present or have defaults.
- AUTO fields are not listed.
- Field types match.
- Unknown fields are rejected.

## UPSERT

Atomically inserts a row or updates it when a declared unique key already exists.

```axis
UPSERT message_receipts
  KEY message_id body.message_id
  KEY user_id auth.user_id
  SET state "read"
  SET read_at NOW
AS receipt
```

At least one `KEY` and one `SET` are required. The complete ordered KEY list must exactly match the shape primary key or one `INDEX ... UNIQUE` declaration. KEY fields cannot also appear in SET, and duplicate KEY or SET fields are compile errors. `AS` binds the actual inserted or updated row on PostgreSQL, MySQL, and SQLite.

UPSERT is a single database statement (`ON CONFLICT ... DO UPDATE` or `ON DUPLICATE KEY UPDATE`), so concurrent writers cannot create a read-before-write race.

## UPDATE

Update existing rows.

### Single-Row Update

```axis
UPDATE bookings
  WHERE id EQ path.id
  WHERE user_id EQ auth.user_id
  SET status cancelled
  SET refund_amount refund
AS updated_booking
OR 404 "booking not found"
```

`AS <name>` binds the updated row.

### Batch Update

```axis
UPDATE bookings
  WHERE listing_id EQ path.listing_id
  WHERE status EQ pending
  SET status cancelled
COUNT updated_count
OR 404
```

`COUNT <name>` binds the number of affected rows.

### OR Clause

`OR <code> [<message>]` is required -- the row might not exist.

## DELETE

Delete rows from a source.

```axis
DELETE bookings
  WHERE id EQ path.id
  WHERE user_id EQ auth.user_id
OR 404 "booking not found"
```

At least one WHERE clause is required. Unqualified DELETE is a compile error. OR is required.

## FANOUT

Bulk-inserts one row per collection item with a single SQL statement.

```axis
FANOUT recipient IN body.recipient_ids
  INSERT delivery_events
    message_id path.id
    recipient_id recipient
    state "pending"
```

The source expression must have a `LIST` type. The item binding exists only inside the nested INSERT. `AS` is intentionally forbidden because a fan-out can produce many rows. FANOUT is supported only for transactional SQL sources and rejects a collection that would exceed the backend's parameter limit (65,535 for PostgreSQL/MySQL and 32,766 for SQLite). An empty list is a successful no-op.

Unlike `EACH ... INSERT`, FANOUT guarantees one bulk statement. Under IDEMPOTENCY it participates in the same transaction as the key reservation, UPSERTs, other mutations, outbox entries, and stored response.

## CALL

Invoke an external service method.

```axis
LET charge
  CALL payments.hold
    amount grand_total
    currency listing.currency
  OR 502 "payment provider unavailable"
```

Arguments are type-checked against the SERVICE METHOD INPUT declaration. OR is required (external services can always fail).

### WASM Call

Call a precompiled WASM module by content hash:

```axis
LET result
  CALL wasm sha256:abc123def456
    input1 input2
  OR 500
```

## EFFECT

Asynchronous side effects represented as durable transactional-outbox records. The outbox record commits with the flow's SQL; delivery or stream publication becomes visible only after a successful commit. This prevents sending a notification for a transaction that later rolls back.

```axis
EFFECT email
  TEMPLATE booking_confirmed
  TO auth.user_id
  DATA booking listing

EFFECT push_notification
  TO listing.host_id
  TEMPLATE new_booking_host
  DATA booking

EFFECT ASYNC reindex
  TASK update_search_index
  DATA listing.id

EFFECT webhook
  URL listing.webhook_url
  EVENT booking.created
  DATA booking
```

### Effect Types

| Type | Description | Required Fields |
|------|-------------|-----------------|
| `email` | Send email via configured provider | TEMPLATE, TO, DATA |
| `push_notification` | Mobile/web push notification | TO, TEMPLATE or EVENT, DATA |
| `ASYNC` | Enqueue background task | TASK, DATA |
| `webhook` | HTTP POST to URL | URL, EVENT, DATA |

### Effect Fields

| Field | Description |
|-------|-------------|
| `TEMPLATE <name>` | Template name for rendering |
| `TO <expr>` | Recipient (user reference resolved to email/device) |
| `DATA <binding1> [binding2...]` | Bindings to pass as payload |
| `URL <expr>` | Webhook URL |
| `EVENT <name>` | Event type name |
| `TASK <name>` | Background task type |

Effects cannot appear before mutations (compiler-enforced ordering). The outbox payload includes a unique id, kind, routing fields, complete JSON data, status, and timestamps so workers can retry delivery safely.

## MATCH

Multi-way branching for conditional logic.

```axis
MATCH
  WHEN GTE hours_until 48
    LET refund
      MUL booking.total_price 1.00
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_type full
    AS updated
    OR 500

  WHEN GTE hours_until 24
    LET refund
      MUL booking.total_price 0.50
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_type half
    AS updated
    OR 500

  DEFAULT
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_type none
    AS updated
    OR 500
```

### Rules

- `WHEN <expr>` -- boolean expression. Evaluated top-to-bottom. First match wins.
- `DEFAULT` -- required unless the compiler can prove exhaustiveness.
- Each branch contains a sequence of operations.
- All branches must produce the same set of bindings for any name used after the MATCH.
- Nested MATCH (MATCH inside MATCH) is a compile error.

## EACH

Iteration over a collection.

```axis
EACH item IN items
  UPDATE inventory
    WHERE product_id EQ item.product_id
    SET stock SUB item.stock item.quantity
  AS updated
  OR 500

EACH user IN users PARALLEL 5
  EFFECT email
    TEMPLATE notification
    TO user.email
    DATA user
```

`EACH <binding> IN <expr> [PARALLEL <count>]`

- `binding` -- name for the current item
- `expr` -- expression resolving to a list
- `PARALLEL <count>` -- optional, process items concurrently with specified parallelism

## TRY / RECOVER

Error recovery. If steps in TRY fail, the RECOVER block runs instead.

```axis
TRY
  LET payment
    CALL payments.charge
      amount total
    OR 502
  SET payment_status "charged"
RECOVER
  SET payment_status "failed"
  EFFECT email
    TEMPLATE payment_failed
    TO auth.user_id
    DATA booking
```

## UPLOAD

Upload a file to a declared STORAGE.

```axis
UPLOAD body.file -> avatars AS upload_url
```

See [STORAGE](constructs/storage.md) for details.

## RETURN

Terminates the flow and defines the HTTP response.

### Return a Binding Directly

```axis
RETURN 200 booking
```

Serializes the full shape. Filtered by SURFACE EXPOSE if applicable.

### Return Inline Fields

```axis
RETURN 201
  id booking.id
  status booking.status
  check_in booking.check_in
  listing
    id listing.id
    title listing.title
```

Nested objects via further indentation.

### Return Paginated List

```axis
RETURN 200
  ITEMS recent_bookings.items
  TOTAL recent_bookings.total
  CURSOR recent_bookings.next_cursor
  HAS_MORE recent_bookings.has_more
```

### Return Empty

```axis
RETURN 204
```

### Response Headers

```axis
RETURN 201 booking
  HEADER Location CONCAT "/bookings/" booking.id
  HEADER X-Idempotency-Key header.idempotency_key
```
