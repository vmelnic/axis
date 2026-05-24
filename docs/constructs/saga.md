# SAGA

Multi-step distributed transaction with compensations. Each step either succeeds or the entire saga rolls back by running compensations in reverse order.

## Syntax

```axis
SAGA <name> <method> <path>
  [REALM <realm_name>]
  [AUTH <auth_spec>]
  [BODY <ShapeName> ...]

  STEP <step_name>
    [<flow_steps>...]
    [VERIFY <expr>]
    [YIELD <binding1> [binding2...]]
    COMPENSATE <NONE | steps...>

  ...

  ON_FAILURE RUN_COMPENSATIONS
  ON_SUCCESS
    [<effects>...]
    RETURN <code> [body]
```

## Example

```axis
SAGA process_booking POST /bookings
  REALM booking_api
  AUTH session
  BODY BookingCreate
    listing_id UUID REQUIRED
    check_in DATE REQUIRED
    check_out DATE REQUIRED

  STEP verify_availability
    LET conflicts
      QUERY bookings
        FILTER listing_id EQ body.listing_id
        FILTER status IN pending confirmed
        FILTER check_in LT body.check_out
        FILTER check_out GT body.check_in
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
    LET hold
      CALL payments.hold
        amount total
        currency listing.currency
      OR 502 "payment hold failed"
    YIELD hold
    COMPENSATE
      CALL payments.refund hold.hold_id
        OR 502

  STEP create_booking
    INSERT bookings
      user_id auth.user_id
      listing_id body.listing_id
      check_in body.check_in
      check_out body.check_out
      status confirmed
      total_price total
      guest_count body.guest_count
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
      TEMPLATE booking_confirmed
      TO auth.user_id
      DATA booking listing
    RETURN 201 booking
```

## STEP

Each STEP contains a sequence of flow operations (LET, FETCH, QUERY, INSERT, UPDATE, DELETE, CALL, GUARD, etc.).

### VERIFY

An optional boolean expression checked after the step's operations. If it evaluates to false, the saga fails at this step.

```axis
STEP check_stock
  LET product
    FETCH products
      FILTER id EQ body.product_id
    OR 404
  VERIFY
    GTE product.stock body.quantity
  YIELD product
  COMPENSATE NONE
```

### YIELD

`YIELD <name1> [name2...]` makes bindings from this step available to subsequent steps. Without YIELD, bindings are local to the step.

### COMPENSATE

Every step must have a COMPENSATE clause:

- `COMPENSATE NONE` -- for read-only steps that need no rollback.
- `COMPENSATE` followed by indented flow steps -- operations to undo this step's effects.

## Execution Semantics

- Steps execute sequentially.
- On failure at step N, compensations run in reverse order: N-1, N-2, ..., 1.
- The step that failed does NOT run its own compensation.
- `ON_FAILURE RUN_COMPENSATIONS` triggers the reverse compensation chain.
- `ON_SUCCESS` runs effects and returns the response on successful completion of all steps.

## Compiler Checks

- Each step must have a COMPENSATE clause.
- Steps with mutations (INSERT, UPDATE, DELETE, CALL) should have non-NONE compensations (warning if not).
- YIELD bindings are verified for existence.
- Sources referenced in steps are verified.
- Storage references in UPLOAD steps are verified.
- The flow's realm capabilities are checked for all operations across all steps.
