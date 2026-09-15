# FLOW

The core construct. A FLOW defines a single HTTP endpoint. Flows are self-contained -- they cannot reference other flows. Cross-service composition uses [SAGA](saga.md).

## Syntax

```axis
FLOW <name> <method> <path>
  [REALM <realm_name>]
  [AUTH <auth_spec>]
  [LIMIT <count> <unit> <scope>]
  [CACHE <ttl> [VARY <fields...>]]
  [SCOPE TENANT <path> | ANY]
  [TIMEOUT <duration>]
  [BODY <ShapeName> ...]
  [PARAM <name> <type> [modifiers...]]
  [HEADER <name> <type> [modifiers...]]
  [IDEMPOTENCY <key_path> SCOPE <scope_path> TTL <seconds>]
  [RULE ...]
  [GUARD ...]
  [LET ...]
  [SET ...]
  [INSERT ... / UPSERT ... / UPDATE ... / DELETE ...]
  [FANOUT ...]
  [EACH ...]
  [TRY ... RECOVER ...]
  [MATCH ...]
  [EFFECT ...]
  [UPLOAD ...]
  RETURN <code> [body]
```

## HTTP Methods

`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `WEBHOOK`

`WEBHOOK` is for receiving webhooks from external services. It is treated as POST for routing but supports webhook-specific auth (HMAC signature verification).

## Example

```axis
FLOW create_booking POST /bookings
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  LIMIT 10 per_minute per_user
  BODY BookingCreate
    listing_id UUID REQUIRED
    check_in DATE REQUIRED
    check_out DATE REQUIRED
    guest_count INT MIN 1 MAX 16 DEFAULT 1
    note MAYBE TEXT

  GUARD valid_dates 400 "check_out must be after check_in"
    GT body.check_out body.check_in

  LET listing
    FETCH listings
      FILTER id EQ body.listing_id
    OR 404 "listing not found"

  GUARD capacity 400 "exceeds max guests"
    LTE body.guest_count listing.max_guests

  LET nights
    DAYS_BETWEEN body.check_in body.check_out

  LET total
    MUL listing.price_per_night nights

  INSERT bookings
    user_id auth.user_id
    listing_id body.listing_id
    check_in body.check_in
    check_out body.check_out
    status pending
    total_price total
    guest_count body.guest_count
  AS booking

  EFFECT email
    TEMPLATE booking_created
    TO auth.user_id
    DATA booking

  RETURN 201 booking
```

## Ordering Rules

The compiler enforces operation ordering within a flow:

1. **Declarations**: AUTH, LIMIT, CACHE, SCOPE, TIMEOUT, BODY, PARAM, HEADER, IDEMPOTENCY (any order among these)
2. **Validations**: RULE, GUARD (must precede mutations)
3. **Computations**: LET, FETCH, QUERY, CALL, MATCH, SET, EACH, TRY (topological order -- can only reference earlier bindings)
4. **Mutations**: INSERT, UPSERT, UPDATE, DELETE, FANOUT (after all validations)
5. **Effects**: EFFECT, UPLOAD (after mutations)
6. **Return**: RETURN (exactly one, always last)

Violations are compile errors. This prevents side effects before validation.

## Flow Operations

All flow operations are documented in detail in [Flow Operations](../flow-operations.md):

- **AUTH** -- authentication and authorization
- **BODY** -- request body declaration
- **PARAM** -- query string parameters
- **HEADER** -- request header declarations
- **IDEMPOTENCY** -- reserve a scoped operation key and replay its exact committed response
- **RULE** -- named authorization checks
- **GUARD** -- inline validation with error codes
- **LET** -- immutable variable binding
- **SET** -- update an existing binding
- **FETCH** -- single-row lookup
- **QUERY** -- multi-row query with filtering and pagination
- **INSERT** -- insert a new row
- **UPSERT** -- atomically insert or update through a declared unique key
- **UPDATE** -- update existing rows
- **DELETE** -- delete rows
- **FANOUT** -- bulk-insert a collection with one SQL statement
- **CALL** -- invoke external service
- **EFFECT** -- asynchronous side effects
- **MATCH** -- multi-way branching
- **EACH** -- iteration over a collection
- **TRY/RECOVER** -- error recovery
- **UPLOAD** -- file upload to storage
- **RETURN** -- response definition

## Atomic Messenger-Style Commands

`IDEMPOTENCY` turns the flow into one database transaction. The key reservation, every SQL read and mutation, transactional outbox writes, and the stored HTTP response commit together. A retry with the same scope, key, and request returns the stored status, body, and headers with `Idempotency-Replayed: true`; the same key with a different request returns `409`.

```axis
FLOW deliver_message POST /messages/:id/deliver
  HEADER idempotency_key STRING 255 REQUIRED
  BODY DeliveryCommand
    actor_id UUID REQUIRED
    recipients LIST UUID REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.actor_id TTL 86400
  UPSERT receipts
    KEY message_id path.id
    KEY user_id body.actor_id
    SET status "delivered"
  AS receipt
  FANOUT recipient IN body.recipients
    INSERT delivery_events
      message_id path.id
      recipient_id recipient
  RETURN 201 receipt
```

An idempotent flow must mutate at least one transactional SQL source. All SQL sources must use one dialect and resolve to the same physical database URL at runtime. Direct service calls, `TRY`, and `UPLOAD` are rejected because they cannot participate in the database transaction; represent external work with `EFFECT`, which writes the transactional outbox.

## Path Parameters

Path segments starting with `:` are extracted as parameters:

```axis
FLOW get_booking GET /bookings/:id
  -- path.id is available as a binding
```

Multiple path parameters:

```axis
FLOW get_review GET /users/:user_id/reviews/:review_id
  -- path.user_id and path.review_id are available
```

## Compiler Checks

- Realm must exist if declared.
- CACHE is only allowed on GET methods.
- SCOPE TENANT ANY requires admin capability.
- All bindings must be defined before use (no forward references).
- Duplicate binding names are rejected.
- All REQUIRED body/param fields are validated.
- INSERT/UPSERT/UPDATE/DELETE/FANOUT source operations are validated against source type.
- Source operations require matching capabilities in the realm.
