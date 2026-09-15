# Axis Language Specification v0.2

## 1. Overview

Axis is a programming language, compiler, and runtime for backend HTTP APIs. It is written exclusively by small LLMs (1B–7B parameters). Humans never read, write, or debug Axis code.

Constraints:
- Frontend exists (React/Vue/Svelte). Axis serves JSON over HTTP.
- Datastores exist (Postgres, Redis, Elasticsearch). Axis queries them; it does not replace them.
- Target authors: 1B, 3B, 7B parameter models with 2K–32K context windows.

**Axis is not a better Python. It is the negation of human-centric programming.**

## 2. Design Principles

1. **Context locality.** A single endpoint must be fully generatable within 512 Axis tokens (~2K LLM tokens with a standard tokenizer). No cross-endpoint references.
2. **No implicit anything.** No hidden imports, ambient authority, global variables, or mutable shared state.
3. **Failure is syntax.** Unhandled errors, missing auth, unindexed queries — these are parse or compile errors, not runtime surprises.
4. **The LLM describes; the runtime executes.** The LLM never writes SQL, retry logic, connection pools, or serialization code. It declares intent.
5. **Programs are Merkle trees.** No files, no directories, no version conflicts. Content-addressed fragments linked by hash.
6. **One operation per binding.** No compound expressions. No operator precedence. Every `LET` binds exactly one operation.
7. **Endpoints cannot call endpoints.** Every flow is self-contained. Cross-service composition uses `SAGA`.

## 3. Lexical Structure

### 3.1 Character Set

UTF-8. Keywords are ASCII uppercase. Identifiers are ASCII lowercase + digits + underscore. Shape names are PascalCase.

### 3.2 Indentation

Significant whitespace: 2 spaces per level. Maximum nesting depth: 4 levels (top-level → block → sub-block → leaf). Tabs are illegal. Trailing whitespace is ignored.

### 3.3 Tokens

```
KEYWORD     = (see §3.4)
IDENT       = [a-z_][a-z0-9_]*
SHAPE_NAME  = [A-Z][a-zA-Z0-9]*
PATH        = "/" [a-z0-9/:_-]+
INT_LIT     = "-"? [0-9]+
DECIMAL_LIT = "-"? [0-9]+ "." [0-9]+
STRING_LIT  = '"' [^"]* '"'
DOT_PATH    = IDENT ("." IDENT)*
NEWLINE     = "\n"
INDENT      = increase in leading spaces (must be +2)
DEDENT      = decrease in leading spaces (must be -2)
COMMENT     = "--" [^\n]*
```

### 3.4 Reserved Keywords

Construct keywords:
```
SHAPE SOURCE REALM FLOW SAGA SURFACE MIGRATE POLICY SERVICE
```

Flow keywords:
```
AUTH BODY PARAM HEADER IDEMPOTENCY RULE GUARD LET FETCH QUERY INSERT UPSERT UPDATE DELETE FANOUT
CALL EFFECT MATCH WHEN DEFAULT RETURN LIMIT CACHE SCOPE REQUIRE KEY
```

Expression keywords:
```
FILTER SORT ASC DESC CURSOR PAGE_SIZE OR AND NOT IF THEN ELSE
EQ NEQ GT GTE LT LTE IN BETWEEN LIKE EMPTY EXISTS
ADD SUB MUL DIV MOD ROUND CEIL FLOOR ABS
COUNT SUM AVG MIN MAX FIRST LAST
CONCAT LOWER UPPER TRIM SUBSTRING LENGTH STARTS_WITH ENDS_WITH CONTAINS
DAYS_BETWEEN HOURS_BETWEEN MINUTES_BETWEEN NOW NOW_PLUS NOW_MINUS FORMAT_DATE
COALESCE LITERAL TO_INT TO_DECIMAL TO_STRING
```

Type keywords:
```
UUID STRING TEXT INT DECIMAL BOOL DATE TIMESTAMP ENUM REF LIST MAP JSON MAYBE
```

Modifier keywords:
```
PK AUTO REQUIRED UNIQUE DEFAULT PRECISION SCALE MIN MAX
```

Other keywords:
```
INDEX TENANT CAPABILITY FIELD HIDE EXPOSE DEPRECATE ROUTE
STEP VERIFY COMPENSATE YIELD ON_SUCCESS ON_FAILURE RUN_COMPENSATIONS
TEMPLATE DATA TASK APPLIES_TO ALL ANY WRITES READS METHOD WHERE SET
COPY COMPUTE DROP NONE TRUE FALSE
HASH ASYNC WEBHOOK SIGNATURE HMAC
ITEMS TOTAL NEXT_CURSOR HAS_MORE
```

New keywords require a spec revision. The keyword set is frozen between major versions.

## 4. Type System

### 4.1 Primitive Types

| Type | Description | Axis literal |
|---|---|---|
| `UUID` | 128-bit identifier | `"550e8400-e29b-..."` |
| `STRING N` | UTF-8 string, max N bytes | `"hello"` |
| `TEXT` | Unbounded UTF-8 string | `"long text..."` |
| `INT` | 64-bit signed integer | `42`, `-1` |
| `DECIMAL P S` | Fixed-point, precision P, scale S | `99.95` |
| `BOOL` | Boolean | `TRUE`, `FALSE` |
| `DATE` | Calendar date (no time) | `"2026-05-08"` |
| `TIMESTAMP` | UTC datetime, microsecond | `"2026-05-08T14:30:00Z"` |

### 4.2 Compound Types

| Type | Description |
|---|---|
| `ENUM v1 v2 ...` | Sum of named variants. Variants are lowercase identifiers. |
| `REF Shape.field` | Foreign key. Compile-time verified against shape. |
| `LIST T` | Ordered collection of type T. |
| `MAP K V` | Key-value pairs. K must be `STRING` or `UUID` or `INT`. |
| `JSON` | Opaque JSON value. Escape hatch. No compile-time checking. |
| `MAYBE T` | Optional value of type T. Must be explicitly unwrapped. |

### 4.3 Type Rules

- No implicit conversions. `INT` does not auto-promote to `DECIMAL`.
- No null. Use `MAYBE T` for optional fields. Use `COALESCE` to unwrap with default.
- `FETCH` returns the shape's type. `FETCH ... OR code` is total (never MAYBE). `FETCH` without `OR` is a compile error.
- `QUERY` returns `LIST Shape`. Always succeeds (may be empty).
- `INSERT` returns the inserted row (same shape as the source).
- `UPDATE` returns the updated row. `UPDATE ... OR code` required (row may not exist).
- `DELETE` returns nothing. `DELETE ... OR code` required.
- Arithmetic: `ADD/SUB/MUL/DIV/MOD` require both operands to be the same numeric type. Result is the same type. `DIV` on `INT` is integer division. Use `TO_DECIMAL` for precise division.
- Comparisons: `EQ/NEQ/GT/GTE/LT/LTE` require same type. Return `BOOL`.
- Boolean: `AND/OR/NOT` require `BOOL`. Return `BOOL`.

### 4.4 Automatic Bindings

These names are available without declaration within a FLOW:

| Name | Type | Available when |
|---|---|---|
| `auth` | Auth shape (see §6.1) | `AUTH` declared |
| `body` | Body shape | `BODY` declared |
| `path` | Record of path params | Path contains `:param` segments |
| `query` | Record of query params | `PARAM` declared |
| `header` | Record of declared headers | `HEADER` declared |

## 5. Constructs

### 5.1 SHAPE

Declares a data structure. Shapes define the schema for sources, request bodies, and response objects.

```axis
SHAPE Booking
  id            UUID        PK AUTO
  user_id       UUID        REF User.id REQUIRED
  listing_id    UUID        REF Listing.id REQUIRED
  check_in      DATE        REQUIRED
  check_out     DATE        REQUIRED
  status        ENUM pending confirmed cancelled completed  REQUIRED
  total_price   DECIMAL     PRECISION 10 SCALE 2 REQUIRED
  guest_count   INT         MIN 1 MAX 16 DEFAULT 1
  note          MAYBE TEXT
  metadata      MAYBE JSON
  created_at    TIMESTAMP   AUTO
  updated_at    TIMESTAMP   AUTO
```

Field modifiers:
- `PK` — primary key. Exactly one per shape.
- `AUTO` — runtime-generated (UUID v7, timestamp, etc). Cannot appear in INSERT.
- `REQUIRED` — must be present. Compile error if INSERT omits it.
- `UNIQUE` — unique constraint. Runtime enforces.
- `DEFAULT value` — default if omitted in INSERT.
- `MIN value` / `MAX value` — bounds for INT/DECIMAL. Compile-time and runtime validated.
- `PRECISION p SCALE s` — for DECIMAL.
- `REF Shape.field` — foreign key. Compiler verifies target shape and field exist.
- `MAYBE` — field is optional. Accessing it without COALESCE on a non-MAYBE binding is a compile error.

### 5.2 SOURCE

Maps a shape to a physical datastore. Every QUERY/FETCH/INSERT/UPSERT/UPDATE/DELETE/FANOUT targets a source, not a shape.

```axis
SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out
  INDEX status

SOURCE listing_cache REDIS
  SHAPE ListingCache
  TTL 3600

SOURCE listing_search ELASTICSEARCH
  SHAPE ListingSearch
  INDEX location GEO
  INDEX title TEXT
  INDEX amenities KEYWORD
```

Source types: `POSTGRES`, `MYSQL`, `REDIS`, `ELASTICSEARCH`, `DYNAMODB`.

Each source type defines which operations are valid:
- `POSTGRES`/`MYSQL`: FETCH, QUERY, INSERT, UPDATE, DELETE
- `REDIS`: FETCH, INSERT (as SET), DELETE. No QUERY.
- `ELASTICSEARCH`: QUERY only. No writes (write through POSTGRES, sync via EFFECT).
- `DYNAMODB`: FETCH, QUERY (with partition key), INSERT, UPDATE, DELETE.

INDEX clause rules:
- Fields listed left-to-right match the index column order.
- Suffix `DESC` on a field for descending order.
- Suffix `GEO` for geo-spatial index.
- Suffix `TEXT` for full-text search index.
- Suffix `KEYWORD` for exact-match on array fields.
- Suffix `UNIQUE` for unique index.
- The compiler verifies every QUERY/FETCH can be served by a declared index. Unindexed access is a compile error with a suggested index.

### 5.3 REALM

Groups sources, capabilities, tenant rules, and policies into a security domain. Every FLOW belongs to exactly one realm.

```axis
REALM booking_api
  TENANT user_id

  CAPABILITY read bookings
  CAPABILITY write bookings
  CAPABILITY read listings
  CAPABILITY read users
  CAPABILITY call payments
  CAPABILITY effect email
  CAPABILITY effect push_notification
```

`TENANT field` declares the row-level isolation field. See §7.12 SCOPE.

`CAPABILITY` declarations whitelist operations. A FLOW in this realm can only access what is listed. The compiler rejects any FETCH/QUERY/INSERT/UPSERT/UPDATE/DELETE/FANOUT/CALL/EFFECT that lacks a matching capability.

Capability types:
- `read source_name` — FETCH and QUERY
- `write source_name` — INSERT, UPDATE, DELETE
- `call service_name` — external CALL
- `effect effect_type` — EFFECT emission

### 5.4 POLICY

Compile-time invariants enforced across all flows in a realm. Policies turn "did you forget X?" from a semantic bug into a compile error.

```axis
POLICY require_auth
  APPLIES_TO FLOW
  REQUIRE AUTH

POLICY require_rate_limit_on_writes
  APPLIES_TO FLOW WHERE METHOD IN post put patch delete
  REQUIRE LIMIT

POLICY require_fraud_check_on_bookings
  APPLIES_TO FLOW WHERE WRITES bookings
  REQUIRE RULE fraud_score_ok

POLICY require_tenant_scope
  APPLIES_TO FLOW WHERE READS bookings
  REQUIRE SCOPE
```

`APPLIES_TO` selectors:
- `FLOW` — all flows in the realm.
- `FLOW WHERE METHOD IN ...` — flows with matching HTTP methods.
- `FLOW WHERE READS source` — flows that FETCH or QUERY the source.
- `FLOW WHERE WRITES source` — flows that INSERT/UPSERT/UPDATE/DELETE/FANOUT the source.
- `FLOW WHERE PATH STARTS_WITH "/admin"` — flows with matching paths.
- `FLOW ALL` followed by an indented selector block — every selector must match.
- `FLOW ANY` followed by an indented selector block — at least one selector must match.
- `NOT` before a block selector — negates that selector.

`REQUIRE` clauses:
- `REQUIRE AUTH` — flow must have an AUTH declaration.
- `REQUIRE AUTH ROLE role_name` — flow must require a specific role.
- `REQUIRE LIMIT` — flow must have at least one LIMIT.
- `REQUIRE SCOPE` — flow must have a SCOPE tenant declaration.
- `REQUIRE RULE rule_name` — flow must include the named rule.
- `REQUIRE GUARD guard_name` — flow must include the named guard.
- `REQUIRE IDEMPOTENCY` — flow must declare atomic idempotency.
- `REQUIRE FANOUT` — flow must contain a FANOUT operation.

The compiler evaluates all policies before code generation. Violations list the policy name, the flow name, and what is missing.

### 5.5 SERVICE

Declares an external service the runtime can CALL. Services are registered, not discovered. The LLM can only CALL declared services.

```axis
SERVICE payments
  ENDPOINT stripe
  AUTH bearer VAULT stripe_api_key

  METHOD hold
    INPUT amount DECIMAL currency STRING
    OUTPUT hold_id STRING status STRING
    TIMEOUT 30s
    RETRY 3 backoff exponential

  METHOD capture
    INPUT hold_id STRING
    OUTPUT transaction_id STRING
    TIMEOUT 30s
    RETRY 3 backoff exponential

  METHOD refund
    INPUT transaction_id STRING amount MAYBE DECIMAL
    OUTPUT refund_id STRING
    TIMEOUT 30s
    RETRY 2 backoff exponential

SERVICE geocoding
  ENDPOINT google_maps
  AUTH query_param VAULT google_maps_key

  METHOD reverse
    PURE
    INPUT lat DECIMAL lng DECIMAL
    OUTPUT address STRING city STRING country STRING
    TIMEOUT 5s
    RETRY 2 backoff linear
    CACHE 86400
```

Service declarations specify:
- `ENDPOINT name` — logical name resolved to a URL by runtime config.
- `AUTH type VAULT key_name` — auth method. `bearer`, `basic`, `query_param`, `header`. Secret resolved from vault at runtime.
- `METHOD name` — an operation the LLM can invoke via `CALL service.method`.
- `PURE` — the method is side-effect-free and repeatable, so retrying an idempotent transaction cannot duplicate external effects.
- `IDEMPOTENCY input_name` — an effectful provider durably deduplicates on this STRING, TEXT, or UUID input. An idempotent flow must bind it directly to its own exact key.
- `INPUT` / `OUTPUT` — typed signatures. The compiler type-checks CALL arguments and YIELD bindings.
- `TIMEOUT` — per-call timeout. Runtime enforces.
- `RETRY count backoff strategy` — retry policy. `exponential`, `linear`, `none`.
- `CACHE seconds` — runtime caches responses by input hash.

### 5.6 FLOW

An HTTP endpoint. The core construct. Self-contained: cannot reference other flows.

```
FLOW <name> <method> <path>
  [REALM realm_name]
  [AUTH ...]
  [LIMIT ...]
  [SCOPE ...]
  [BODY ...]
  [PARAM ...]
  [HEADER ...]
  [IDEMPOTENCY key_path SCOPE scope_path TTL seconds]
  [RULE ...]
  [GUARD ...]
  [LET ...]
  [FETCH/QUERY ...]
  [INSERT/UPSERT/UPDATE/DELETE/FANOUT ...]
  [CALL ...]
  [MATCH ...]
  [EFFECT ...]
  RETURN code [body]
```

Methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `WEBHOOK`.

Ordering rules (compiler-enforced):
1. Declaration block: AUTH, LIMIT, SCOPE, BODY, PARAM, HEADER, IDEMPOTENCY (any order)
2. Validation block: RULE, GUARD (must precede mutations)
3. Computation block: LET, FETCH, QUERY, CALL, MATCH (topological order — can only reference earlier bindings)
4. Mutation block: INSERT, UPSERT, UPDATE, DELETE, FANOUT (after all validations)
5. Effect block: EFFECT (after mutations)
6. Return: RETURN (exactly one, always last)

Violations of this ordering are compile errors. This prevents side effects before validation — a structural guarantee, not a convention.

`UPSERT` requires KEY fields that exactly match a primary key or declared UNIQUE index. `FANOUT` accepts a typed LIST and emits one bulk INSERT statement. `IDEMPOTENCY` reserves `(flow, scope, key)`, executes all SQL and outbox writes, stores the response, and commits them as one transaction. Identical retries replay the exact committed response; a different request with the same key returns 409. Idempotent flows cannot span physical databases or SQL dialects and cannot contain TRY or UPLOAD. Direct service calls must be `PURE` or declare provider `IDEMPOTENCY` and receive the exact flow key.

### 5.7 SAGA

Multi-step distributed transaction with compensations. Each step either succeeds or the entire saga rolls back by running compensations in reverse order.

```axis
SAGA process_booking POST /bookings
  REALM booking_api
  AUTH session
  BODY BookingCreate
    listing_id  UUID  REQUIRED
    check_in    DATE  REQUIRED
    check_out   DATE  REQUIRED

  STEP verify_availability
    LET conflicts
      QUERY bookings
        FILTER listing_id EQ body.listing_id
        FILTER status     IN pending confirmed
        FILTER check_in   LT body.check_out
        FILTER check_out  GT body.check_in
    VERIFY
      EMPTY conflicts
    COMPENSATE NONE

  STEP calculate_price
    LET listing
      FETCH listings
        FILTER id EQ body.listing_id
      OR 404
    LET nights
      DAYS_BETWEEN body.check_in body.check_out
    LET total
      MUL listing.price_per_night nights
    YIELD listing total
    COMPENSATE NONE

  STEP hold_payment
    CALL payments.hold amount total currency listing.currency
    YIELD hold_id
    COMPENSATE
      CALL payments.refund hold_id

  STEP create_booking
    INSERT bookings
      user_id       auth.user_id
      listing_id    body.listing_id
      check_in      body.check_in
      check_out     body.check_out
      status        confirmed
      total_price   total
    YIELD booking
    COMPENSATE
      DELETE bookings
        WHERE id EQ booking.id

  STEP capture_payment
    CALL payments.capture hold_id
    YIELD transaction_id
    COMPENSATE
      CALL payments.refund transaction_id

  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    EFFECT email
      TEMPLATE booking_confirmed
      TO auth.user_id
      DATA booking
    EFFECT push_notification
      TO listing.host_id
      DATA booking
    RETURN 201 booking
```

Saga rules:
- Steps execute sequentially.
- Each step must have a `COMPENSATE` clause (`NONE` for read-only steps).
- `YIELD name` makes a binding available to subsequent steps.
- On failure at step N, compensations run in reverse: N-1, N-2, ..., 1.
- The runtime persists saga state to a durable journal. On process crash, saga resumes from last completed step.
- The runtime generates idempotency keys for each step. Retries are safe.

### 5.8 SURFACE

Maps internal flows to versioned external API contracts. Decouples internal evolution from client compatibility.

```axis
SURFACE public v1
  REALM booking_api
  BASE_PATH /api/v1

  ROUTE GET  /bookings       -> list_bookings
  ROUTE POST /bookings       -> create_booking
  ROUTE GET  /bookings/:id   -> get_booking

  EXPOSE Booking AS BookingResponse
    FIELD id         UUID
    FIELD status     STRING
    FIELD check_in   DATE
    FIELD check_out  DATE
    HIDE total_price
    HIDE user_id
    HIDE metadata

SURFACE public v2
  REALM booking_api
  BASE_PATH /api/v2

  ROUTE GET  /bookings       -> list_bookings_v2
  ROUTE POST /bookings       -> create_booking
  ROUTE GET  /bookings/:id   -> get_booking_v2

  EXPOSE Booking AS BookingResponse
    FIELD id          UUID
    FIELD status      STRING
    FIELD check_in    DATE
    FIELD check_out   DATE
    FIELD total_price DECIMAL
    FIELD guest_count INT
    RENAME note AS special_requests
    HIDE user_id
    HIDE metadata

  DEPRECATE v1 SUNSET 2027-06-01
```

Surface rules:
- `BASE_PATH` prefixes all routes. Client sees `/api/v1/bookings`, runtime dispatches to `list_bookings`.
- `EXPOSE Shape AS Name` defines the external contract. Only listed fields are serialized. `HIDE` explicitly excludes fields.
- `RENAME field AS external_name` maps internal field names to external ones.
- `DEPRECATE version SUNSET date` — runtime adds `Deprecation` and `Sunset` headers to responses on the deprecated surface. After the sunset date, the runtime returns 410 Gone.
- The compiler verifies every EXPOSE field exists in the underlying shape with a compatible type.
- The runtime auto-generates OpenAPI 3.1 specs from surface definitions.

### 5.9 MIGRATE

Schema evolution. Transforms a shape from one version to another.

```axis
MIGRATE Booking v1 TO v2
  COPY id user_id listing_id check_in check_out status created_at
  COMPUTE total_price
    IF
      EXISTS v1.total_price
      THEN v1.total_price
      ELSE MUL v1.nights v1.price_per_night
  DROP nights
  DROP price_per_night
  ADD cancellation_policy ENUM flexible moderate strict DEFAULT flexible
  ADD updated_at TIMESTAMP DEFAULT NOW
```

Migration operations:
- `COPY field...` — carry forward unchanged.
- `COMPUTE field expr` — derive new field from old fields.
- `DROP field` — remove field. Data is archived, not deleted.
- `ADD field type modifiers` — new field with default.
- `RENAME old_field TO new_field` — rename without data change.

The runtime:
- Generates SQL DDL (ALTER TABLE) from the migration.
- Runs backfill in batches for COMPUTE fields.
- Validates the migration is reversible (stores the inverse).
- Applies atomically: either all changes succeed or none do.
- Blocks deployment of flows referencing v2 shape until migration completes.

## 6. Flow Operations

### 6.1 AUTH

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

Auth types:
- `none` — no authentication. Must be explicit (compiler error if omitted, per POLICY).
- `session` — cookie-based session. `auth.user_id`, `auth.email`, etc. available.
- `bearer` — JWT bearer token. Claims available as `auth.*`.
- `api_key` — API key in header. `auth.key_id`, `auth.scopes` available.
- `role name` — requires the specified role. Shorthand for session + role check.
- `role IN name...` — requires any of the listed roles.
- `WEBHOOK SIGNATURE` — validates request signature (e.g., Stripe webhooks). `secret_name` resolved from vault. `HMAC sha256` specifies algorithm.

The runtime resolves auth before the flow executes. Auth failure returns 401 (unauthenticated) or 403 (unauthorized) before any flow operation runs.

### 6.2 BODY, PARAM, HEADER

Declare input shapes.

```axis
BODY BookingCreate
  listing_id  UUID      REQUIRED
  check_in    DATE      REQUIRED
  check_out   DATE      REQUIRED
  guest_count INT       MIN 1 MAX 16 DEFAULT 1
  note        MAYBE TEXT

PARAM page      INT     DEFAULT 1 MIN 1
PARAM page_size INT     DEFAULT 20 MIN 1 MAX 100
PARAM status    MAYBE ENUM pending confirmed cancelled
PARAM sort_by   ENUM created_at price DEFAULT created_at
PARAM sort_dir  ENUM asc desc DEFAULT desc

HEADER idempotency_key UUID REQUIRED
HEADER accept_language STRING DEFAULT "en"
```

`BODY` fields are parsed from the request body (JSON). `PARAM` fields are parsed from query string. `HEADER` fields are parsed from request headers.

Type validation is automatic: if `guest_count` arrives as `"abc"`, the runtime returns 400 with a structured error before the flow executes. MIN/MAX bounds are runtime-validated.

### 6.3 RULE

Named authorization checks. Rules are reusable across flows and can be required by POLICY.

```axis
RULE user_may_book
  REQUIRE auth.email_verified EQ TRUE
  REQUIRE auth.account_status EQ active
  REQUIRE auth.ban_status     NEQ banned

RULE is_listing_owner
  REQUIRE auth.user_id EQ listing.host_id

RULE fraud_score_ok
  REQUIRE auth.fraud_score LTE 50
```

Rules fail with 403. Each `REQUIRE` clause is a boolean assertion on available bindings. All must be true.

Rules can reference bindings declared earlier in the flow (e.g., `listing` must be FETCHed before `is_listing_owner` can check it). The compiler verifies binding availability.

### 6.4 GUARD

Inline validation. Guards short-circuit the flow with an error.

```axis
GUARD valid_dates 400 "check_out must be after check_in"
  GT body.check_out body.check_in

GUARD no_overlap 409 "dates are unavailable"
  EMPTY
    QUERY bookings
      FILTER listing_id EQ body.listing_id
      FILTER status     IN pending confirmed
      FILTER check_in   LT body.check_out
      FILTER check_out  GT body.check_in

GUARD guest_capacity 400 "exceeds max guests"
  LTE body.guest_count listing.max_guests
```

Syntax: `GUARD name http_code [message]` followed by a boolean expression on the next indented line(s). If the expression evaluates to false, the flow returns the error code and message immediately.

### 6.5 LET

Bind a name to the result of a single operation. Immutable. One operation per LET.

```axis
LET nights
  DAYS_BETWEEN body.check_in body.check_out

LET base_price
  MUL listing.price_per_night nights

LET has_weekly_discount
  AND
    listing.weekly_discount
    GTE nights 7

LET total
  IF has_weekly_discount
    THEN MUL base_price 0.90
    ELSE base_price

LET service_fee
  MUL total 0.12

LET grand_total
  ADD total service_fee
```

Bindings are available to all subsequent operations in the flow. Forward references are compile errors.

### 6.6 FETCH and QUERY

#### FETCH — single row

```axis
LET booking
  FETCH bookings
    FILTER id EQ path.id
  OR 404

LET listing
  FETCH listings
    FILTER id EQ booking.listing_id
  OR 500 "listing not found for booking"
```

`FETCH` returns exactly one row. If zero rows match, the `OR` clause fires. If multiple rows match, the runtime returns the first (by PK order) and logs a warning. The compiler verifies `OR` is present.

#### QUERY — multiple rows

```axis
LET recent_bookings
  QUERY bookings
    FILTER user_id  EQ auth.user_id
    FILTER status   IN confirmed completed
    FILTER check_in GTE NOW_MINUS 90 days
    SORT created_at DESC
    CURSOR query.cursor
    PAGE_SIZE query.page_size
```

QUERY clauses:
- `FILTER field OP value` — where condition. Multiple FILTERs are AND-ed.
- `SORT field ASC|DESC` — order by. Multiple SORT clauses for compound ordering.
- `CURSOR value` — opaque cursor for keyset pagination. Omit for first page.
- `PAGE_SIZE value` — max rows to return. Required if CURSOR is used.

Filter operators: `EQ`, `NEQ`, `GT`, `GTE`, `LT`, `LTE`, `IN`, `BETWEEN`, `LIKE`, `STARTS_WITH`, `CONTAINS`.

QUERY always succeeds (returns empty list on no matches). No `OR` clause.

The compiler verifies every QUERY is covered by a declared INDEX. If the FILTER/SORT combination cannot be served by any index, the compiler rejects with a suggested index.

#### Pagination response

When CURSOR and PAGE_SIZE are used, QUERY returns a pagination wrapper:

```
{
  "items": [...],
  "total": 142,
  "next_cursor": "eyJpZCI6...",
  "has_more": true
}
```

In the flow, access via `recent_bookings.items`, `recent_bookings.total`, `recent_bookings.next_cursor`, `recent_bookings.has_more`.

### 6.7 INSERT, UPDATE, DELETE

#### INSERT

```axis
INSERT bookings
  user_id       auth.user_id
  listing_id    body.listing_id
  check_in      body.check_in
  check_out     body.check_out
  status        pending
  total_price   grand_total
  guest_count   body.guest_count
AS booking
```

`AS name` binds the inserted row (including AUTO fields like `id`, `created_at`). The compiler verifies all REQUIRED fields are present and types match. AUTO fields must not be listed.

#### UPDATE

```axis
UPDATE bookings
  WHERE id EQ path.id
  WHERE user_id EQ auth.user_id
  SET status cancelled
  SET cancelled_at NOW
  SET refund_amount refund
AS updated_booking
OR 404 "booking not found"
```

`WHERE` clauses identify the row(s). Multiple WHERE clauses are AND-ed. `SET` clauses assign new values. `AS name` binds the updated row. `OR` is required — the row might not exist.

For batch updates (multiple rows):

```axis
UPDATE bookings
  WHERE listing_id EQ path.listing_id
  WHERE status EQ pending
  SET status cancelled
COUNT updated_count
```

`COUNT name` binds the number of affected rows instead of a single row.

#### DELETE

```axis
DELETE bookings
  WHERE id EQ path.id
  WHERE user_id EQ auth.user_id
OR 404 "booking not found"
```

DELETE requires at least one WHERE clause. Unqualified DELETE (no WHERE) is a compile error. `OR` is required.

### 6.8 CALL

Invoke an external service method. The service must be declared (§5.5) and the realm must have a matching CAPABILITY.

```axis
LET charge
  CALL payments.hold
    amount   grand_total
    currency listing.currency
  OR 502 "payment provider unavailable"

LET geocoded
  CALL geocoding.reverse
    lat body.latitude
    lng body.longitude
  OR 502 "geocoding failed"
```

The compiler type-checks arguments against the SERVICE METHOD INPUT declaration. The runtime handles timeouts, retries, and circuit breaking per the service definition.

`OR` is required on CALL (external services can always fail).

### 6.9 EFFECT

Asynchronous side effects are appended to the durable outbox inside the flow's database transaction. Workers deliver committed records with retries; no external effect becomes visible for a rolled-back transaction.

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

Effect types:
- `email` — email via configured provider. `TEMPLATE` names a template. `TO` is a user reference (runtime resolves to email). `DATA` lists bindings to pass.
- `push_notification` — mobile/web push.
- `ASYNC` — enqueue a background task. `TASK` names the task type. The runtime manages the task queue.
- `webhook` — HTTP POST to a URL. `EVENT` names the event type. `DATA` is serialized as JSON body.

Effects cannot appear before mutations (compiler-enforced ordering). This prevents sending a "booking confirmed" email when the INSERT hasn't happened yet.

The runtime:
- Persists effects to an outbox table within the same DB transaction as mutations.
- Processes the outbox asynchronously with at-least-once semantics.
- Logs every effect with a content-addressed trace ID.
- Retries failed effects with exponential backoff (configurable per effect type).

### 6.10 MATCH

Multi-way branching. For conditional logic with more than two paths.

```axis
LET hours_until
  HOURS_BETWEEN NOW booking.check_in

MATCH
  WHEN GTE hours_until 48
    LET refund
      MUL booking.total_price 1.00
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_amount refund
      SET refund_type full
    AS updated
    OR 500
    CALL payments.refund
      transaction_id booking.charge_id
      amount refund
    OR 502

  WHEN GTE hours_until 24
    LET refund
      MUL booking.total_price 0.50
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_amount refund
      SET refund_type half
    AS updated
    OR 500
    CALL payments.refund
      transaction_id booking.charge_id
      amount refund
    OR 502

  DEFAULT
    UPDATE bookings
      WHERE id EQ path.id
      SET status cancelled
      SET refund_amount 0
      SET refund_type none
    AS updated
    OR 500
```

MATCH rules:
- `WHEN expr` — boolean expression. Evaluated top-to-bottom. First match wins.
- `DEFAULT` — required unless the compiler can prove exhaustiveness (e.g., ENUM matching).
- Each branch contains a sequence of operations (LET, UPDATE, CALL, etc).
- All branches must produce the same set of bindings for any name used after the MATCH. If branch A binds `updated` and branch B doesn't, and `updated` is referenced after the MATCH, it's a compile error.
- Nesting depth: MATCH adds one level. MATCH inside MATCH is a compile error (max depth enforced).

### 6.11 RETURN

Terminates the flow and defines the HTTP response.

```axis
-- Return a single object
RETURN 201
  id          booking.id
  status      booking.status
  check_in    booking.check_in
  check_out   booking.check_out
  total_price booking.total_price
  listing
    id        listing.id
    title     listing.title

-- Return a binding directly
RETURN 200 booking

-- Return a paginated list
RETURN 200
  ITEMS   recent_bookings.items
  TOTAL   recent_bookings.total
  CURSOR  recent_bookings.next_cursor
  HAS_MORE recent_bookings.has_more

-- Return empty
RETURN 204
```

RETURN shapes:
- **Inline projection**: indented `field value` pairs. Nested objects via further indentation. The compiler verifies all referenced bindings exist and types resolve.
- **Direct binding**: `RETURN code name` — serializes the full shape. Filtered by SURFACE EXPOSE if applicable.
- **Paginated list**: uses ITEMS/TOTAL/CURSOR/HAS_MORE keywords.
- **Empty**: just the status code.

Response headers can be set:

```axis
RETURN 201 booking
  HEADER Location CONCAT "/bookings/" booking.id
  HEADER X-Idempotency-Key header.idempotency_key
```

### 6.12 LIMIT, CACHE, SCOPE

#### LIMIT

```axis
LIMIT 10 per_minute per_user
LIMIT 100 per_hour per_user
LIMIT 1000 per_hour global
```

Rate limiting. `per_user` scopes by `auth.user_id`. `per_ip` scopes by source IP. `global` is system-wide. Runtime implements via token bucket. Multiple LIMIT clauses stack (all must pass).

#### CACHE

Two forms: flow-level caching and inline query caching.

Flow-level: cache the entire response.

```axis
FLOW get_listing GET /listings/:id
  CACHE 60 VARY path.id
  CACHE 300 VARY path.id auth.user_id
```

`CACHE seconds VARY field...` — cache key includes the listed fields. Multiple CACHE declarations: shortest TTL wins if same VARY, different VARY creates separate cache entries.

Inline: cache a specific query result.

```axis
LET popular
  CACHE 300
    QUERY listings
      FILTER status EQ active
      SORT booking_count DESC
      PAGE_SIZE 10
```

Runtime caches by content hash of the query + VARY fields. Automatic invalidation when the underlying source is modified (tracked via the outbox).

Only GET flows may use flow-level CACHE. The compiler rejects CACHE on POST/PUT/PATCH/DELETE.

#### SCOPE

```axis
SCOPE TENANT auth.user_id
SCOPE TENANT ANY
```

Tenant scoping. When declared, the runtime injects `FILTER tenant_field EQ scope_value` into every QUERY and FETCH on tenanted sources within the flow. This injection happens at the query planner level — the LLM never sees it, but it's guaranteed to be applied.

`SCOPE TENANT ANY` bypasses tenant filtering. Requires `CAPABILITY admin` in the realm. Used for admin/support flows.

If a realm declares `TENANT`, the compiler requires every flow in that realm to have a `SCOPE TENANT` declaration (enforceable via POLICY).

## 7. Expressions Reference

### 7.1 Arithmetic

| Op | Signature | Notes |
|---|---|---|
| `ADD a b` | `(Num, Num) → Num` | Same numeric type required |
| `SUB a b` | `(Num, Num) → Num` | |
| `MUL a b` | `(Num, Num) → Num` | `MUL INT DECIMAL` is a compile error. Use `TO_DECIMAL`. |
| `DIV a b` | `(Num, Num) → Num` | INT division truncates. DECIMAL divides exactly. |
| `MOD a b` | `(INT, INT) → INT` | |
| `ROUND a scale` | `(DECIMAL, INT) → DECIMAL` | Round to N decimal places. |
| `CEIL a` | `(DECIMAL) → INT` | |
| `FLOOR a` | `(DECIMAL) → INT` | |
| `ABS a` | `(Num) → Num` | |

### 7.2 Comparison

| Op | Signature | Notes |
|---|---|---|
| `EQ a b` | `(T, T) → BOOL` | Same type required |
| `NEQ a b` | `(T, T) → BOOL` | |
| `GT a b` | `(Ord, Ord) → BOOL` | Ord = INT, DECIMAL, DATE, TIMESTAMP, STRING |
| `GTE a b` | `(Ord, Ord) → BOOL` | |
| `LT a b` | `(Ord, Ord) → BOOL` | |
| `LTE a b` | `(Ord, Ord) → BOOL` | |
| `IN a vals...` | `(T, T...) → BOOL` | `IN status pending confirmed` |
| `BETWEEN a lo hi` | `(Ord, Ord, Ord) → BOOL` | Inclusive both ends |

### 7.3 Boolean

| Op | Signature |
|---|---|
| `AND a b` | `(BOOL, BOOL) → BOOL` |
| `OR a b` | `(BOOL, BOOL) → BOOL` |
| `NOT a` | `(BOOL) → BOOL` |

For multi-argument AND/OR, use indented form:

```axis
LET eligible
  AND
    auth.email_verified
    EQ auth.account_status active
    GTE auth.trust_score 50
```

Indented AND/OR takes 2+ arguments (all must be BOOL).

### 7.4 String

| Op | Signature | Example |
|---|---|---|
| `CONCAT a b...` | `(STRING...) → STRING` | `CONCAT "Hello " user.name` |
| `LOWER a` | `(STRING) → STRING` | |
| `UPPER a` | `(STRING) → STRING` | |
| `TRIM a` | `(STRING) → STRING` | |
| `SUBSTRING a start len` | `(STRING, INT, INT) → STRING` | |
| `LENGTH a` | `(STRING) → INT` | |
| `STARTS_WITH a prefix` | `(STRING, STRING) → BOOL` | |
| `ENDS_WITH a suffix` | `(STRING, STRING) → BOOL` | |
| `CONTAINS a substr` | `(STRING, STRING) → BOOL` | |

### 7.5 Date/Time

| Op | Signature | Example |
|---|---|---|
| `NOW` | `() → TIMESTAMP` | |
| `NOW_PLUS n unit` | `(INT, Unit) → TIMESTAMP` | `NOW_PLUS 7 days` |
| `NOW_MINUS n unit` | `(INT, Unit) → TIMESTAMP` | `NOW_MINUS 30 days` |
| `DAYS_BETWEEN a b` | `(DATE, DATE) → INT` | |
| `HOURS_BETWEEN a b` | `(TIMESTAMP, TIMESTAMP) → INT` | |
| `MINUTES_BETWEEN a b` | `(TIMESTAMP, TIMESTAMP) → INT` | |
| `FORMAT_DATE a fmt` | `(DATE/TIMESTAMP, STRING) → STRING` | `FORMAT_DATE booking.check_in "YYYY-MM-DD"` |

Units: `seconds`, `minutes`, `hours`, `days`, `weeks`, `months`, `years`.

### 7.6 Aggregate

Aggregates operate on QUERY results or bindings of type LIST.

| Op | Signature | Example |
|---|---|---|
| `COUNT query` | `(LIST) → INT` | `COUNT bookings` |
| `SUM query field` | `(LIST, Field) → Num` | `SUM bookings total_price` |
| `AVG query field` | `(LIST, Field) → DECIMAL` | `AVG reviews rating` |
| `MIN query field` | `(LIST, Field) → T` | `MIN bookings check_in` |
| `MAX query field` | `(LIST, Field) → T` | `MAX bookings check_out` |
| `FIRST query` | `(LIST) → MAYBE T` | `FIRST sorted_listings` |
| `LAST query` | `(LIST) → MAYBE T` | |

### 7.7 Control

| Op | Syntax | Notes |
|---|---|---|
| `IF cond THEN a ELSE b` | `(BOOL, T, T) → T` | Both branches must return same type. ELSE is required. |
| `COALESCE a default` | `(MAYBE T, T) → T` | Unwrap optional with fallback. |
| `EMPTY query` | `(LIST) → BOOL` | True if list has zero elements. |
| `EXISTS query` | `(LIST) → BOOL` | True if list has one or more elements. |

### 7.8 Conversion

| Op | Signature |
|---|---|
| `TO_INT a` | `(DECIMAL/STRING) → INT` |
| `TO_DECIMAL a` | `(INT/STRING) → DECIMAL` |
| `TO_STRING a` | `(INT/DECIMAL/BOOL/DATE/TIMESTAMP/UUID) → STRING` |
| `LITERAL value` | `() → inferred type` |

## 8. Formal Grammar (EBNF)

```ebnf
program         = { construct } ;
construct       = shape | source | realm | policy | service | flow | saga | surface | migrate ;

(* --- Shapes --- *)
shape           = "SHAPE" SHAPE_NAME NL INDENT { field_def } DEDENT ;
field_def       = IDENT type_expr { modifier } NL ;
type_expr       = "UUID" | "BOOL" | "DATE" | "TIMESTAMP" | "TEXT"
                | "STRING" INT_LIT
                | "INT" [ "MIN" INT_LIT ] [ "MAX" INT_LIT ]
                | "DECIMAL" "PRECISION" INT_LIT "SCALE" INT_LIT
                | "ENUM" IDENT { IDENT }
                | "REF" SHAPE_NAME "." IDENT
                | "LIST" type_expr
                | "MAP" type_expr type_expr
                | "JSON"
                | "MAYBE" type_expr ;
modifier        = "PK" | "AUTO" | "REQUIRED" | "UNIQUE"
                | "DEFAULT" literal
                | "PRECISION" INT_LIT | "SCALE" INT_LIT
                | "MIN" INT_LIT | "MAX" INT_LIT ;

(* --- Sources --- *)
source          = "SOURCE" IDENT source_type NL INDENT
                    "SHAPE" SHAPE_NAME NL
                    { index_def }
                    [ "TTL" INT_LIT NL ]
                  DEDENT ;
source_type     = "POSTGRES" | "MYSQL" | "REDIS" | "ELASTICSEARCH" | "DYNAMODB" ;
index_def       = "INDEX" IDENT { IDENT } [ index_suffix ] NL ;
index_suffix    = "ASC" | "DESC" | "GEO" | "TEXT" | "KEYWORD" | "UNIQUE" ;

(* --- Realms --- *)
realm           = "REALM" IDENT NL INDENT
                    [ "TENANT" IDENT NL ]
                    { capability_def }
                  DEDENT ;
capability_def  = "CAPABILITY" cap_type IDENT NL ;
cap_type        = "read" | "write" | "call" | "effect" | "admin" ;

(* --- Policies --- *)
policy          = "POLICY" IDENT NL INDENT
                    applies_to
                    { require_clause }
                  DEDENT ;
applies_to      = "APPLIES_TO" "FLOW"
                  ( [ "WHERE" policy_cond { policy_cond } ] NL
                  | ( "ALL" | "ANY" ) NL INDENT
                      { [ "NOT" ] policy_cond NL }
                    DEDENT ) ;
policy_cond     = "METHOD" "IN" IDENT { IDENT }
                | "READS" IDENT
                | "WRITES" IDENT
                | "PATH" "STARTS_WITH" STRING_LIT ;
require_clause  = "REQUIRE" require_target NL ;
require_target  = "AUTH" [ "ROLE" IDENT ]
                | "LIMIT"
                | "SCOPE"
                | "RULE" IDENT
                | "GUARD" IDENT
                | "IDEMPOTENCY"
                | "FANOUT" ;

(* --- Services --- *)
service         = "SERVICE" IDENT NL INDENT
                    "ENDPOINT" IDENT NL
                    "AUTH" svc_auth_type "VAULT" IDENT NL
                    { method_def }
                  DEDENT ;
svc_auth_type   = "bearer" | "basic" | "query_param" | "header" ;
method_def      = "METHOD" IDENT NL INDENT
                    "INPUT" { IDENT type_expr } NL
                    "OUTPUT" { IDENT type_expr } NL
                    [ "TIMEOUT" duration NL ]
                    [ "PURE" NL ]
                    [ "RETRY" INT_LIT "backoff" retry_strat NL ]
                    [ "CACHE" INT_LIT NL ]
                  DEDENT ;
retry_strat     = "exponential" | "linear" | "none" ;
duration        = INT_LIT ("s" | "ms") ;

(* --- Flows --- *)
flow            = "FLOW" IDENT http_method PATH NL INDENT
                    [ "REALM" IDENT NL ]
                    { flow_decl }
                    { flow_step }
                    return_stmt
                  DEDENT ;
http_method     = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "WEBHOOK" ;
flow_decl       = auth_decl | limit_decl | scope_decl
                | body_decl | param_decl | header_decl | idempotency_decl ;
auth_decl       = "AUTH" auth_spec NL ;
auth_spec       = "none" | "session" | "bearer" | "api_key"
                | "role" IDENT
                | "role" "IN" IDENT { IDENT }
                | "WEBHOOK" "SIGNATURE" IDENT "HMAC" IDENT ;
limit_decl      = "LIMIT" INT_LIT rate_unit rate_scope NL ;
rate_unit       = "per_second" | "per_minute" | "per_hour" | "per_day" ;
rate_scope      = "per_user" | "per_ip" | "per_key" | "global" ;
scope_decl      = "SCOPE" "TENANT" ( dot_path | "ANY" ) NL ;
body_decl       = "BODY" SHAPE_NAME NL INDENT { field_def } DEDENT ;
param_decl      = "PARAM" IDENT type_expr { modifier } NL ;
header_decl     = "HEADER" IDENT type_expr { modifier } NL ;
idempotency_decl = "IDEMPOTENCY" dot_path "SCOPE" dot_path "TTL" INT_LIT NL ;

flow_step       = rule_step | guard_step | let_step
                | fetch_step | query_step
                | insert_step | upsert_step | update_step | delete_step | fanout_step
                | call_step | effect_step | match_step ;

rule_step       = "RULE" IDENT NL INDENT { require_line } DEDENT ;
require_line    = "REQUIRE" dot_path compare_op expr NL ;

guard_step      = "GUARD" IDENT INT_LIT [ STRING_LIT ] NL INDENT expr NL DEDENT ;

let_step        = "LET" IDENT NL INDENT expr NL DEDENT ;

fetch_step      = "LET" IDENT NL INDENT
                    "FETCH" IDENT NL INDENT { filter_clause } DEDENT
                    "OR" INT_LIT [ STRING_LIT ] NL
                  DEDENT ;

query_step      = "LET" IDENT NL INDENT
                    [ "CACHE" INT_LIT NL ]
                    "QUERY" IDENT NL INDENT
                      { filter_clause }
                      { sort_clause }
                      [ cursor_clause ]
                      [ page_size_clause ]
                    DEDENT
                  DEDENT ;

filter_clause   = "FILTER" IDENT filter_op expr NL ;
filter_op       = "EQ" | "NEQ" | "GT" | "GTE" | "LT" | "LTE"
                | "IN" | "BETWEEN" | "LIKE" | "STARTS_WITH" | "CONTAINS" ;
sort_clause     = "SORT" IDENT ( "ASC" | "DESC" ) NL ;
cursor_clause   = "CURSOR" dot_path NL ;
page_size_clause = "PAGE_SIZE" ( dot_path | INT_LIT ) NL ;

insert_step     = "INSERT" IDENT NL INDENT
                    { IDENT expr NL }
                  DEDENT
                  [ "AS" IDENT NL ] ;

upsert_step     = "UPSERT" IDENT NL INDENT
                    { "KEY" IDENT expr NL }
                    { "SET" IDENT expr NL }
                  DEDENT
                  [ "AS" IDENT NL ] ;

fanout_step     = "FANOUT" IDENT "IN" expr NL INDENT
                    insert_step
                  DEDENT ;

update_step     = "UPDATE" IDENT NL INDENT
                    { where_clause }
                    { set_clause }
                  DEDENT
                  ( "AS" IDENT NL | "COUNT" IDENT NL )
                  "OR" INT_LIT [ STRING_LIT ] NL ;

delete_step     = "DELETE" IDENT NL INDENT
                    { where_clause }
                  DEDENT
                  "OR" INT_LIT [ STRING_LIT ] NL ;

where_clause    = "WHERE" IDENT compare_op expr NL ;
set_clause      = "SET" IDENT expr NL ;
compare_op      = "EQ" | "NEQ" | "GT" | "GTE" | "LT" | "LTE" | "IN" ;

call_step       = "LET" IDENT NL INDENT
                    "CALL" dot_path NL INDENT { IDENT expr NL } DEDENT
                    "OR" INT_LIT [ STRING_LIT ] NL
                  DEDENT ;

effect_step     = "EFFECT" effect_type NL INDENT { effect_field } DEDENT ;
effect_type     = "email" | "push_notification" | "ASYNC" | "webhook" ;
effect_field    = ( "TEMPLATE" | "TO" | "DATA" | "URL" | "EVENT" | "TASK" ) expr NL ;

match_step      = "MATCH" NL INDENT
                    { when_branch }
                    [ default_branch ]
                  DEDENT ;
when_branch     = "WHEN" expr NL INDENT { flow_step } DEDENT ;
default_branch  = "DEFAULT" NL INDENT { flow_step } DEDENT ;

return_stmt     = "RETURN" INT_LIT [ return_body ] NL ;
return_body     = IDENT
                | NL INDENT { return_field } DEDENT ;
return_field    = IDENT expr NL
                | IDENT NL INDENT { return_field } DEDENT
                | "HEADER" IDENT expr NL
                | "ITEMS" expr NL
                | "TOTAL" expr NL
                | "CURSOR" expr NL
                | "HAS_MORE" expr NL ;

(* --- Sagas --- *)
saga            = "SAGA" IDENT http_method PATH NL INDENT
                    [ "REALM" IDENT NL ]
                    { flow_decl }
                    { saga_step }
                    on_failure
                    on_success
                  DEDENT ;
saga_step       = "STEP" IDENT NL INDENT
                    { flow_step }
                    [ "VERIFY" NL INDENT expr NL DEDENT ]
                    [ "YIELD" IDENT { IDENT } NL ]
                    compensate
                  DEDENT ;
compensate      = "COMPENSATE" ( "NONE" | NL INDENT { flow_step } DEDENT ) NL ;
on_failure      = "ON_FAILURE" "RUN_COMPENSATIONS" NL ;
on_success      = "ON_SUCCESS" NL INDENT
                    { effect_step }
                    return_stmt
                  DEDENT ;

(* --- Surfaces --- *)
surface         = "SURFACE" IDENT IDENT NL INDENT
                    "REALM" IDENT NL
                    "BASE_PATH" PATH NL
                    { route_def }
                    { expose_def }
                    [ deprecate_def ]
                  DEDENT ;
route_def       = "ROUTE" http_method PATH "->" IDENT NL ;
expose_def      = "EXPOSE" SHAPE_NAME "AS" SHAPE_NAME NL INDENT
                    { expose_field }
                  DEDENT ;
expose_field    = "FIELD" IDENT type_expr NL
                | "HIDE" IDENT NL
                | "RENAME" IDENT "AS" IDENT NL ;
deprecate_def   = "DEPRECATE" IDENT "SUNSET" STRING_LIT NL ;

(* --- Migrations --- *)
migrate         = "MIGRATE" SHAPE_NAME IDENT "TO" IDENT NL INDENT
                    { migrate_op }
                  DEDENT ;
migrate_op      = "COPY" IDENT { IDENT } NL
                | "COMPUTE" IDENT NL INDENT expr NL DEDENT
                | "DROP" IDENT NL
                | "ADD" IDENT type_expr { modifier } NL
                | "RENAME" IDENT "TO" IDENT NL ;

(* --- Shared --- *)
expr            = literal | dot_path | unary_expr | binary_expr | call_expr | if_expr ;
unary_expr      = ( "NOT" | "EMPTY" | "EXISTS" | "COUNT" | "LOWER" | "UPPER"
                  | "TRIM" | "ABS" | "CEIL" | "FLOOR" | "NOW" | "LENGTH"
                  | "FIRST" | "LAST" | "TO_INT" | "TO_DECIMAL" | "TO_STRING" )
                  expr ;
binary_expr     = ( "ADD" | "SUB" | "MUL" | "DIV" | "MOD" | "AND" | "OR"
                  | "EQ" | "NEQ" | "GT" | "GTE" | "LT" | "LTE"
                  | "CONCAT" | "STARTS_WITH" | "ENDS_WITH" | "CONTAINS"
                  | "DAYS_BETWEEN" | "HOURS_BETWEEN" | "MINUTES_BETWEEN"
                  | "COALESCE" | "ROUND" | "SUBSTRING" | "FORMAT_DATE"
                  | "SUM" | "AVG" | "MIN" | "MAX" )
                  expr expr [ expr ] ;
call_expr       = "CALL" dot_path { IDENT expr } ;
if_expr         = "IF" expr "THEN" expr "ELSE" expr ;
literal         = INT_LIT | DECIMAL_LIT | STRING_LIT | "TRUE" | "FALSE"
                | "NONE" | "NOW" | now_offset | IDENT ;
now_offset      = ( "NOW_PLUS" | "NOW_MINUS" ) INT_LIT time_unit ;
time_unit       = "seconds" | "minutes" | "hours" | "days" | "weeks" | "months" | "years" ;
dot_path        = IDENT { "." IDENT } ;
```

## 9. Compilation Model

### 9.1 Pipeline

```
Source text → [Lexer] → Tokens → [Parser] → AST → [Linker] → Linked AST
→ [Verifier] → Verified AST → [Planner] → Execution Plan → [Codegen] → Target
```

### 9.2 Stage 1: Lex

Converts source text to token stream. Handles indentation tracking (INDENT/DEDENT tokens emitted on level changes). Rejects:
- Tabs
- Indentation not a multiple of 2
- Unrecognized tokens

### 9.3 Stage 2: Parse

Builds AST from token stream. The grammar (§8) is LL(1) — parseable with a single token of lookahead. This enables constrained decoding: at any point during generation, the set of valid next tokens is deterministic.

Rejects:
- Syntax errors
- Nesting depth > 4

### 9.4 Stage 3: Link

Resolves references by content hash. Every SHAPE_NAME, source name, service name, realm name is resolved.

Rejects:
- Undefined shape references
- Undefined source references
- Undefined service/method references
- Circular dependencies between shapes (REF cycles are allowed; structural cycles are not)

### 9.5 Stage 4: Verify

Type checking and semantic analysis.

**Type check:**
- Every expression is type-checked bottom-up.
- No implicit conversions.
- MAYBE types must be unwrapped via COALESCE before use in non-MAYBE context.
- RETURN shape must match SURFACE EXPOSE if a surface maps to this flow.

**Capability check:**
- Every FETCH/QUERY on source S requires `CAPABILITY read S` in the flow's realm.
- Every INSERT/UPSERT/UPDATE/DELETE/FANOUT on source S requires `CAPABILITY write S`.
- Every CALL to service S requires `CAPABILITY call S`.
- Every EFFECT of type T requires `CAPABILITY effect T`.

**Index check:**
- For each QUERY/FETCH, the compiler extracts the set of filter fields and sort fields.
- It matches against declared INDEXes on the source (leftmost prefix matching).
- If no index covers the query, the compiler rejects and outputs: `ERROR: QUERY on source 'bookings' with filters [listing_id, status, check_in, check_out] is not covered by any index. Suggested: INDEX listing_id status check_in check_out`.

**Totality check:**
- Every FETCH has an OR clause.
- Every UPDATE has an OR clause.
- Every DELETE has an OR clause.
- Every CALL has an OR clause.
- Every IF has THEN and ELSE.
- Every MATCH has DEFAULT (unless compiler proves exhaustiveness).
- Every SAGA STEP has COMPENSATE.

**Tenant check:**
- If the realm declares TENANT, every flow accessing a tenanted source must declare SCOPE TENANT.
- SCOPE TENANT ANY requires CAPABILITY admin.

**Policy check:**
- All POLICY REQUIRE clauses are evaluated against every matching flow.
- Violations are compile errors listing the policy, the flow, and the missing element.

**Ordering check:**
- Operations appear in the correct phase: declarations → validations → computations → mutations → effects → return.
- Effects cannot precede mutations.
- Mutations cannot precede guards.
- LET bindings are topologically ordered (no forward references).

**Surface check:**
- Every EXPOSE FIELD exists in the underlying shape with a compatible type.
- Every ROUTE target flow exists.
- RENAME fields must exist in the shape.
- HIDE fields must exist in the shape.

### 9.6 Stage 5: Plan

Optimization and execution planning.

**Query planning:**
- Detect N+1 patterns: if a FETCH inside (or after) a QUERY references `query_result.field`, batch into a single IN query.
- Merge redundant queries: if two QUERYs on the same source with the same filters exist, deduplicate.
- Select index: for each query, choose the optimal index (most selective prefix match).
- Choose execution strategy: SQL for relational, Redis protocol for cache, ES DSL for search.

**Parallel execution:**
- Analyze the dependency graph of LET bindings.
- Bindings with no data dependency on each other can execute in parallel.
- Example: `LET listing` (FETCH) and `LET reviews` (QUERY) that both depend on `path.id` but not on each other → parallel execution.

**Transaction planning:**
- All mutations within a FLOW are wrapped in a single database transaction.
- Effects are written to the outbox table within the same transaction.
- For SAGAs, each STEP is a separate transaction with journal entries.

### 9.7 Stage 6: Codegen

Emit target artifact.

**Target A — Native binary (Rust):**
- Generates a Rust module per flow.
- Compiles to a single binary with embedded HTTP server (hyper/axum), connection pools (sqlx/deadpool), and effect processor.
- Output: statically linked binary + OpenAPI spec JSON.

**Target B — WASM module:**
- Generates WASM component per flow.
- Runs on Cloudflare Workers, Fastly Compute, or any WASI-compatible runtime.
- Database access via bound service connections.
- Output: .wasm file + OpenAPI spec JSON.

**Target C — Interpreted:**
- Generates a JSON execution plan consumed by a generic Axis runtime process.
- Hot-reloadable: swap the plan without restarting the process.
- Useful for development and rapid iteration.
- Output: execution plan JSON + OpenAPI spec JSON.

All targets auto-generate:
- OpenAPI 3.1 specification from FLOW + SURFACE definitions.
- Prometheus metrics endpoints.
- Health check endpoints (`/healthz`, `/readyz`).
- Structured JSON request logs.

## 10. Runtime Specification

### 10.1 Request Lifecycle

```
HTTP Request
  │
  ├─ [1] Route Match ─── Surface → Flow resolution
  │                       404 if no match
  │
  ├─ [2] Rate Limit ──── Token bucket check
  │                       429 if exceeded (Retry-After header)
  │
  ├─ [3] Parse Input ──── Body/Param/Header deserialization + type validation
  │                       400 if invalid (structured error body)
  │
  ├─ [4] Auth ─────────── Session/JWT/API key/Webhook signature verification
  │                       401 if missing, 403 if insufficient
  │
  ├─ [5] Tenant Scope ─── Inject tenant filter into query context
  │
  ├─ [6] Rules ────────── Authorization checks
  │                       403 if failed
  │
  ├─ [7] Guards ───────── Validation checks
  │                       4xx per guard definition
  │
  ├─ [8] Execute ──────── LET bindings, FETCH, QUERY, CALL, MATCH
  │                       DB transaction opened on first mutation
  │
  ├─ [9] Mutate ───────── INSERT/UPSERT/UPDATE/DELETE/FANOUT within transaction
  │                       Effects written to outbox within same transaction
  │                       Transaction committed
  │
  ├─ [10] Effects ─────── Outbox processor delivers asynchronously
  │
  └─ [11] Respond ─────── Surface filter → JSON serialization → HTTP response
```

### 10.2 Error Response Format

All errors follow a consistent structure:

```json
{
  "error": {
    "code": 409,
    "type": "guard_failed",
    "guard": "no_overlap",
    "message": "dates are unavailable",
    "trace_id": "01HYX3K..."
  }
}
```

Types: `validation_error`, `auth_error`, `rule_failed`, `guard_failed`, `not_found`, `conflict`, `rate_limited`, `internal_error`, `service_unavailable`.

### 10.3 Observability

Every request produces a structured trace:

```json
{
  "trace_id": "01HYX3K...",
  "flow": "create_booking",
  "surface": "public/v2",
  "method": "POST",
  "path": "/api/v2/bookings",
  "auth": { "user_id": "...", "role": "guest" },
  "tenant": "user_123",
  "duration_ms": 47,
  "steps": [
    { "op": "guard", "name": "valid_dates", "result": "pass", "ms": 0 },
    { "op": "guard", "name": "no_overlap", "result": "pass", "ms": 3, "query_ms": 3 },
    { "op": "fetch", "source": "listings", "result": "found", "ms": 2 },
    { "op": "fetch", "source": "users", "result": "found", "ms": 1 },
    { "op": "guard", "name": "host_active", "result": "pass", "ms": 0 },
    { "op": "insert", "source": "bookings", "result": "ok", "ms": 5 },
    { "op": "effect", "type": "email", "template": "booking_request", "queued": true }
  ],
  "response": { "code": 201 },
  "queries": [
    { "source": "bookings", "sql": "SELECT ... WHERE ...", "ms": 3, "rows": 0 },
    { "source": "listings", "sql": "SELECT ... WHERE id = $1", "ms": 2, "rows": 1 },
    { "source": "users", "sql": "SELECT ... WHERE id = $1", "ms": 1, "rows": 1 }
  ]
}
```

The runtime emits:
- **Structured logs**: JSON per request (above format).
- **Metrics**: Prometheus counters/histograms per flow, per source, per service call.
  - `axis_request_duration_seconds{flow, method, status}`
  - `axis_query_duration_seconds{source, index_used}`
  - `axis_call_duration_seconds{service, method, status}`
  - `axis_effect_queue_depth{type}`
  - `axis_saga_step_duration_seconds{saga, step, status}`
- **Traces**: OpenTelemetry spans. Each flow step is a child span.

### 10.4 Configuration

The runtime is configured via a declarative config file, not Axis code:

```yaml
axis:
  version: "0.2"
  manifest: "sha256:abc123..."

sources:
  bookings:
    type: postgres
    url: "${DATABASE_URL}"
    pool_size: 20
    statement_timeout: 5s
  listing_cache:
    type: redis
    url: "${REDIS_URL}"
    pool_size: 10
  listing_search:
    type: elasticsearch
    url: "${ELASTICSEARCH_URL}"

services:
  payments:
    endpoint: "https://api.stripe.com/v1"
  geocoding:
    endpoint: "https://maps.googleapis.com/maps/api"

vault:
  provider: env
  # or: aws_secrets_manager, hashicorp_vault

auth:
  session:
    type: jwt
    issuer: "${AUTH_ISSUER}"
    audience: "${AUTH_AUDIENCE}"
    jwks_url: "${AUTH_JWKS_URL}"
  api_key:
    header: "X-API-Key"
    source: api_keys_table

effects:
  email:
    provider: sendgrid
    api_key: "${SENDGRID_API_KEY}"
  push_notification:
    provider: firebase
    credentials: "${FIREBASE_CREDENTIALS}"
  outbox:
    poll_interval: 1s
    max_retries: 5
    backoff: exponential

server:
  port: 8080
  graceful_shutdown: 30s
  request_timeout: 30s
  cors:
    origins: ["https://app.example.com"]
    methods: ["GET", "POST", "PUT", "PATCH", "DELETE"]
    headers: ["Authorization", "Content-Type"]
    max_age: 3600
```

The LLM never sees or generates this config. It is human-authored and environment-specific.

## 11. Orchestrator Protocol

The orchestrator bridges natural language requirements and Axis code generation. It is not part of the Axis language — it is the integration layer.

### 11.1 Architecture

```
Natural Language Requirement
  │
  ├─ [Decomposer] ─── 70B+ model or human
  │   Breaks requirement into a task manifest:
  │   shapes, sources, flows, sagas, surfaces
  │
  ├─ [Context Assembler] ─── deterministic
  │   For each task, assembles a prompt with:
  │   - Relevant shapes (only those referenced)
  │   - Relevant sources (only those accessed)
  │   - Relevant services (only those called)
  │   - Task description
  │   - One few-shot example of the same construct type
  │
  ├─ [Generator] ─── 1B/3B/7B model
  │   Generates Axis code fragment with constrained decoding
  │
  ├─ [Compiler] ─── deterministic
  │   Compiles fragment. On error:
  │   - Feed error + original code back to generator
  │   - Max 3 retries before escalating
  │
  ├─ [Linker] ─── deterministic
  │   Links all compiled fragments into manifest
  │
  └─ [Deployer] ─── deterministic
      Generates target artifact and deploys
```

### 11.2 Prompt Format

The generator receives a structured prompt:

```
-- CONTEXT

SHAPE User
  id            UUID      PK AUTO
  email         STRING 255 REQUIRED UNIQUE
  name          STRING 100 REQUIRED
  host_status   ENUM active inactive
  email_verified BOOL DEFAULT FALSE
  account_status ENUM active suspended banned DEFAULT active
  fraud_score   INT MIN 0 MAX 100 DEFAULT 0

SHAPE Listing
  id                UUID      PK AUTO
  host_id           UUID      REF User.id REQUIRED
  title             STRING 200 REQUIRED
  price_per_night   DECIMAL   PRECISION 10 SCALE 2 REQUIRED
  max_guests        INT       MIN 1 MAX 16 REQUIRED
  weekly_discount   BOOL      DEFAULT FALSE
  status            ENUM active paused deleted REQUIRED

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out

SOURCE listings POSTGRES
  SHAPE Listing
  INDEX host_id
  INDEX status price_per_night

REALM booking_api
  TENANT user_id
  CAPABILITY read bookings
  CAPABILITY write bookings
  CAPABILITY read listings
  CAPABILITY read users

-- TASK

Generate a FLOW: Cancel a booking. User can cancel their own booking.
Refund policy: full refund if >48h before check-in, 50% if 24-48h, none if <24h.
Update booking status to cancelled with refund amount.

-- EXAMPLE

FLOW get_booking GET /bookings/:id
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  GUARD ownership 403 "not your booking"
    EQ booking.user_id auth.user_id
  RETURN 200 booking
```

### 11.3 Constrained Decoding

The parser's LL(1) property enables constrained decoding during generation:

At each token position, the parser computes the set of valid next tokens. The LLM's logits are masked to only allow valid tokens. This guarantees:
- Every generated program is syntactically valid.
- No retry needed for syntax errors.
- Generation is faster (smaller valid token set → higher confidence per token).

The constrained decoder maintains a parser state machine that advances with each generated token.

### 11.4 Error Recovery

When the compiler rejects generated code (type errors, missing indexes, policy violations), the error is fed back:

```
-- ERROR

Your generated FLOW cancel_booking has errors:
1. TYPE_ERROR line 14: MUL requires same numeric types.
   Got: MUL booking.total_price 0.50
   booking.total_price is DECIMAL(10,2), 0.50 is untyped.
   Fix: use LITERAL 0.50 or ensure both are DECIMAL.
2. POLICY_VIOLATION: Policy 'require_rate_limit_on_writes' requires LIMIT
   on FLOW cancel_booking (method DELETE).
   Fix: add a LIMIT declaration.

-- ORIGINAL CODE

FLOW cancel_booking DELETE /bookings/:id
  ...

-- TASK

Fix the errors above and regenerate the FLOW.
```

Max 3 retries. If still failing, escalate to a larger model or flag for human review.

## 12. Testing Model

Axis programs are tested without human-written test code.

### 12.1 Property-Based Testing

The compiler generates property tests from type signatures and constraints:

- **Type invariant**: For all valid inputs matching the BODY schema, the response matches the RETURN shape or is a declared error code.
- **Auth invariant**: Requests without valid auth receive 401 or 403, never 2xx.
- **Tenant invariant**: A request with SCOPE TENANT user_A never returns data where tenant_field ≠ user_A.
- **Idempotency**: For flows with IDEMPOTENCY, concurrent identical requests execute once and return the same committed status, body, and headers; key reuse with different input returns 409.
- **Guard invariant**: For inputs that violate a GUARD condition, the response is the GUARD's error code.

### 12.2 Trace Replay

Production traces (§10.3) can be replayed against a new version of the flow:

1. Capture request trace from production (input + auth context + timestamp).
2. Replay against new flow version with mocked external services (CALL results from original trace).
3. Compare response shape and status code.
4. Flag regressions: different status code, missing fields, type changes.

### 12.3 Contract Testing

SURFACE definitions are contracts. The compiler generates contract tests:

- For each ROUTE in a SURFACE, verify the flow exists and its RETURN shape is compatible with the EXPOSE definition.
- For each DEPRECATE, verify the replacement surface covers all routes.
- Generate HTTP-level contract tests (request/response pairs) that can run against a live instance.

### 12.4 Chaos Testing

The runtime supports fault injection for CALL and QUERY operations:

```yaml
chaos:
  enabled: true
  rules:
    - target: "call:payments.*"
      failure_rate: 0.1
      failure_type: timeout
    - target: "query:bookings"
      delay_ms: 500
      probability: 0.05
```

Verifies saga compensations trigger correctly and effects don't fire on failed transactions.

## 13. Deployment Model

### 13.1 Content-Addressed Manifests

```
manifest: sha256:abc123...
├── realm: sha256:def456...
│   ├── shapes
│   │   ├── Booking@v2: sha256:aaa111...
│   │   ├── Listing@v1: sha256:bbb222...
│   │   └── User@v1: sha256:ccc333...
│   ├── sources
│   │   ├── bookings: sha256:ddd444...
│   │   └── listings: sha256:eee555...
│   ├── services
│   │   └── payments: sha256:fff666...
│   ├── policies
│   │   ├── require_auth: sha256:ggg777...
│   │   └── require_rate_limit: sha256:hhh888...
│   ├── flows
│   │   ├── create_booking: sha256:iii999...
│   │   ├── get_booking: sha256:jjj000...
│   │   └── cancel_booking: sha256:kkk111...
│   ├── sagas
│   │   └── process_booking: sha256:lll222...
│   └── surfaces
│       ├── public/v1: sha256:mmm333...
│       └── public/v2: sha256:nnn444...
└── config_schema: sha256:ooo555...
```

### 13.2 Deployment Operations

**Deploy**: Compile all fragments → link into manifest → verify → generate target → swap manifest hash atomically.

**Rollback**: Point to previous manifest hash. Instant. No rebuild needed (artifacts are cached by hash).

**Canary**: Run two manifest versions simultaneously. Route a percentage of traffic to the new manifest. Compare error rates and latencies.

**Blue-green**: Two complete runtime instances. Switch DNS/load balancer after verification.

### 13.3 Migration Coordination

When a deployment includes MIGRATE operations:

1. Runtime enters "migration mode": new manifest is compiled but not activated.
2. Migration runs in background (batched, resumable).
3. Both old and new flows are available during migration (old flows read old schema, new flows wait).
4. Migration completes → atomic switchover to new manifest.
5. Old schema is retained for rollback window (configurable, default 7 days).

## 14. The 1B / 3B / 7B Spectrum

### 14.1 1B Model (2K–4K context)

**Can generate:**
- SHAPE definitions
- Simple CRUD FLOWs (GET by id, POST create, DELETE)
- Basic GUARD clauses (single condition)
- RULE with 1–2 REQUIRE clauses
- Simple RETURN (direct binding or 2–3 inline fields)

**Cannot generate:**
- SAGAs
- MATCH blocks
- Complex aggregations
- SURFACE/MIGRATE definitions
- Multi-step business logic (>5 LET bindings)

**Strategy:** Template-based generation. The orchestrator provides a skeleton with holes. The 1B model fills entity names, field names, filter conditions.

**Context budget:**
- System prompt + few-shot: ~500 tokens
- Referenced shapes (2–3): ~150 tokens
- Sources + realm: ~100 tokens
- Task description: ~100 tokens
- Generation: ~200 tokens (~60–80 Axis tokens)

```axis
FLOW get_booking GET /bookings/:id
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id

  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404

  GUARD ownership 403 "not your booking"
    EQ booking.user_id auth.user_id

  RETURN 200 booking
```

### 14.2 3B Model (4K–8K context)

**Can generate:**
- Everything 1B can, plus:
- Multi-step FLOWs with 5–15 LET bindings
- Aggregations (COUNT, SUM, AVG)
- Conditional logic (IF/THEN/ELSE in expressions)
- QUERY with FILTER + SORT + CURSOR
- Multiple GUARDs and RULEs
- Conditional EFFECTs

**Cannot generate:**
- SAGAs with >3 steps
- Complex MATCH with side effects in branches
- MIGRATE with COMPUTE expressions
- SURFACE with RENAME/complex EXPOSE

**Context budget:**
- System prompt + few-shot: ~800 tokens
- Referenced shapes (3–5): ~250 tokens
- Sources + realm + services: ~200 tokens
- Task description: ~200 tokens
- Generation: ~600 tokens (~120–200 Axis tokens)

```axis
FLOW host_dashboard GET /host/dashboard
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  PARAM period INT DEFAULT 30 MIN 1 MAX 365
  LIMIT 30 per_minute per_user
  CACHE 60 VARY auth.user_id query.period

  LET listings
    QUERY listings
      FILTER host_id EQ auth.user_id
      FILTER status EQ active

  LET bookings
    QUERY bookings
      FILTER listing_id IN listings.id
      FILTER created_at GTE NOW_MINUS period days

  LET confirmed
    QUERY bookings
      FILTER listing_id IN listings.id
      FILTER status EQ confirmed
      FILTER created_at GTE NOW_MINUS period days

  LET listing_count
    COUNT listings

  LET confirmed_count
    COUNT confirmed

  LET capacity
    MUL listing_count period

  LET occupancy
    IF GT capacity 0
      THEN DIV
        TO_DECIMAL confirmed_count
        TO_DECIMAL capacity
      ELSE LITERAL 0.0

  LET revenue
    SUM confirmed total_price

  LET avg_rating
    AVG listings avg_rating

  RETURN 200
    listing_count  listing_count
    booking_count  COUNT bookings
    confirmed      confirmed_count
    occupancy      occupancy
    revenue        revenue
    avg_rating     avg_rating
    period         period
```

### 14.3 7B Model (8K–32K context)

**Can generate:**
- Everything 3B can, plus:
- SAGAs with 5+ steps and compensations
- MATCH with multi-branch side effects
- SURFACE definitions with EXPOSE/HIDE/RENAME
- MIGRATE with COMPUTE expressions
- Complex SERVICE CALL flows
- Multi-source queries with cross-referencing

**Cannot generate:**
- Arbitrary algorithms (sorting, graph traversal, ML inference)
- Custom binary protocols
- Complex string parsing/transformation

**Strategy:** For algorithms, reference WASM modules by hash: `CALL wasm sha256:xyz... input`.

**Context budget:**
- System prompt + few-shot: ~1000 tokens
- Referenced shapes (5–10): ~500 tokens
- Sources + realm + services + policies: ~400 tokens
- Task description: ~300 tokens
- Generation: ~2000 tokens (~300–500 Axis tokens)

```axis
SAGA instant_book POST /bookings/instant
  REALM booking_api
  AUTH session
  BODY InstantBookRequest
    listing_id  UUID  REQUIRED
    check_in    DATE  REQUIRED
    check_out   DATE  REQUIRED
    guest_count INT   MIN 1 MAX 16 DEFAULT 1

  STEP load_listing
    LET listing
      FETCH listings
        FILTER id EQ body.listing_id
        FILTER status EQ active
      OR 404 "listing not found or inactive"
    VERIFY
      EQ listing.instant_book TRUE
    YIELD listing
    COMPENSATE NONE

  STEP check_availability
    LET conflicts
      QUERY bookings
        FILTER listing_id EQ body.listing_id
        FILTER status     IN pending confirmed
        FILTER check_in   LT body.check_out
        FILTER check_out  GT body.check_in
    VERIFY
      EMPTY conflicts
    YIELD conflicts
    COMPENSATE NONE

  STEP calculate_price
    LET nights
      DAYS_BETWEEN body.check_in body.check_out
    LET base
      MUL listing.price_per_night nights
    LET has_discount
      AND
        listing.weekly_discount
        GTE nights 7
    LET total
      IF has_discount
        THEN MUL base 0.90
        ELSE base
    LET service_fee
      ROUND
        MUL total 0.12
        2
    LET grand_total
      ADD total service_fee
    YIELD total service_fee grand_total
    COMPENSATE NONE

  STEP hold_payment
    LET hold
      CALL payments.hold
        amount   grand_total
        currency listing.currency
      OR 502 "payment hold failed"
    YIELD hold
    COMPENSATE
      CALL payments.refund hold.hold_id
        OR 502

  STEP create_booking
    INSERT bookings
      user_id       auth.user_id
      listing_id    body.listing_id
      check_in      body.check_in
      check_out     body.check_out
      status        confirmed
      total_price   grand_total
      guest_count   body.guest_count
    AS booking
    YIELD booking
    COMPENSATE
      DELETE bookings
        WHERE id EQ booking.id
      OR 500

  STEP capture_payment
    LET captured
      CALL payments.capture hold.hold_id
      OR 502 "payment capture failed"
    YIELD captured
    COMPENSATE
      CALL payments.refund captured.transaction_id
        OR 502

  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    EFFECT email
      TEMPLATE instant_booking_confirmed
      TO auth.user_id
      DATA booking listing
    EFFECT email
      TEMPLATE new_instant_booking
      TO listing.host_id
      DATA booking
    EFFECT ASYNC
      TASK update_calendar
      DATA listing.id booking.check_in booking.check_out
    EFFECT ASYNC
      TASK update_search_index
      DATA listing.id
    RETURN 201
      id           booking.id
      status       booking.status
      check_in     booking.check_in
      check_out    booking.check_out
      total_price  grand_total
      service_fee  service_fee
      listing
        id         listing.id
        title      listing.title
```

## 15. Honest Limitations

1. **The query planner is the hardest piece.** Generating optimal SQL from FILTER/SORT/CURSOR across Postgres, Redis, and Elasticsearch is a multi-year engineering effort. Start with Postgres-only, add other backends incrementally.

2. **Semantic bugs persist.** Axis prevents structural bugs (null derefs, unhandled errors, SQL injection, unindexed queries, missing auth, missing tenant scope) but not logical bugs. A 3B model can write `EQ booking.user_id listing.host_id` when it should be `EQ booking.user_id auth.user_id`. POLICY reduces but does not eliminate this class.

3. **WASM escape hatch required.** Custom algorithms (image processing, ML inference, recommendation ranking, custom crypto) cannot be expressed in Axis. `CALL wasm sha256:xyz...` lets flows invoke precompiled modules. These modules are human-written or generated by larger models.

4. **Real-time is out of scope.** WebSockets, Server-Sent Events, long-polling — none of these fit the request/response model. Axis serves HTTP APIs. Real-time features need a separate service that reads from the same databases.

5. **Schema design requires a bigger model.** A 1B–3B model can fill in flows given shapes, but designing the right shapes for a complex domain requires understanding the whole domain simultaneously. Schema design is a 7B+ or human task.

6. **Observability requires human tools.** The runtime produces traces, but interpreting them for debugging requires human-facing tools: trace viewers, query analyzers, saga state inspectors. These must be built.

7. **Constrained decoding is implementation-specific.** The LL(1) grammar enables it, but integrating constrained decoding with each LLM inference framework (vLLM, llama.cpp, TensorRT-LLM) requires per-framework work.

8. **Cold start on WASM targets.** WASM modules have measurable cold start latency on serverless platforms. For latency-critical flows, native binary is preferred.
