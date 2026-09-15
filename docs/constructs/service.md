# SERVICE

Declares an external service the runtime can CALL. Services are registered, not discovered -- a flow can only call services that are explicitly declared.

## Syntax

```axis
SERVICE <name>
  ENDPOINT <logical_name>
  AUTH <type> VAULT <key_name>

  METHOD <name>
    [PURE]
    [IDEMPOTENCY <input_name>]
    INPUT <param1> <type1> [param2 type2 ...]
    OUTPUT <param1> <type1> [param2 type2 ...]
    [TIMEOUT <duration>]
    [RETRY <count> backoff <strategy>]
    [CACHE <seconds>]
```

## Example

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
  AUTH bearer VAULT google_maps_key

  METHOD reverse
    INPUT lat DECIMAL lng DECIMAL
    OUTPUT address STRING city STRING country STRING
    TIMEOUT 5s
    RETRY 2 backoff linear
    CACHE 86400
```

## ENDPOINT

`ENDPOINT <name>` is a logical name resolved to a URL by runtime configuration. The actual URL is never in Axis code.

## AUTH

`AUTH <type> VAULT <key_name>` specifies the authentication method and the secret key name.

Auth types: `bearer`, `basic`, `query_param`, `header`.

The secret is resolved from the vault at runtime (environment variables by default).

## METHOD

Each METHOD declares an operation that flows can invoke via `CALL service.method`.

### PURE

`PURE` declares that a method has no externally observable side effects and that the same inputs can be evaluated repeatedly. Axis therefore permits it in flows that also declare `IDEMPOTENCY`; a failed transaction may safely evaluate the call again. False declarations break the service contract: use `PURE` only for validation, deterministic derivation, and read-only computation. Capability checks, timeout handling, and failure handling still apply.

```axis
METHOD verify_signature
  PURE
  INPUT public_key TEXT payload TEXT signature TEXT
  OUTPUT valid BOOL
TIMEOUT 3s
```

### IDEMPOTENCY

`IDEMPOTENCY <input_name>` declares an effectful provider method with durable
deduplication. The named `STRING`, `TEXT`, or `UUID` input is the provider's
operation key: repeated calls with that key must return the same logical result
without repeating the external effect.

```axis
METHOD send_otp
  IDEMPOTENCY operation_id
  INPUT operation_id UUID phone_ciphertext TEXT
  OUTPUT delivery_id STRING 128 code_hash STRING 128 expires_at TIMESTAMP
```

In a flow that also declares `IDEMPOTENCY`, Axis permits this call only when the
named method argument is bound directly to the flow's exact idempotency key.
This covers the failure window where the provider succeeds and the SQL
transaction later rolls back: the retry observes the provider's stored result
and can safely commit locally. `PURE` and method `IDEMPOTENCY` are mutually
exclusive. A false declaration violates the service adapter contract.

### INPUT / OUTPUT

Typed parameter signatures. The compiler type-checks CALL arguments against INPUT and binds return values against OUTPUT.

```axis
METHOD hold
  INPUT amount DECIMAL currency STRING
  OUTPUT hold_id STRING status STRING
```

### TIMEOUT

Per-call timeout. The runtime enforces this and returns an error if exceeded.

```axis
TIMEOUT 30s      -- 30 seconds
TIMEOUT 500ms    -- 500 milliseconds
```

Duration units: `ms` (milliseconds), `s` (seconds), `m` (minutes), `h` (hours).

### RETRY

Retry policy with backoff strategy.

```axis
RETRY 3 backoff exponential
RETRY 2 backoff linear
```

Strategies:
- `exponential` -- exponential backoff between retries
- `linear` -- linear backoff between retries
- `none` -- no backoff

The runtime retries on 5xx errors but not on 4xx errors.

### CACHE

Response caching by input hash.

```axis
CACHE 86400    -- cache for 24 hours
```

## Usage in Flows

Services are invoked with `CALL` in a flow or saga step:

```axis
LET charge
  CALL payments.hold
    amount grand_total
    currency listing.currency
  OR 502 "payment provider unavailable"
```

The realm must have `CAPABILITY call <service_name>` for the service to be callable.

## Compiler Checks

- Service and method names are verified when referenced in CALL.
- CALL arguments are type-checked against METHOD INPUT.
- CALL always requires an OR clause (external services can always fail).
- An idempotent mutating flow may call a `PURE` method or a provider-deduplicated method whose declared `IDEMPOTENCY` input receives the exact flow key. Other effects must use the transactional outbox. Direct calls can run while the SQL transaction is open, so they should be fast and have strict timeouts.
- The flow's realm must have `CAPABILITY call <service_name>`.
