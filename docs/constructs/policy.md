# POLICY

Compile-time invariants enforced across matching flows. Policies turn a missing reliability or security declaration into a compiler error.

## Syntax

The compact form keeps backwards-compatible single-line selectors:

```axis
POLICY authenticated_writes
  APPLIES_TO FLOW WHERE METHOD IN post put patch delete
  REQUIRE AUTH
```

The block form supports several selectors, explicit `ALL`/`ANY` matching, and `NOT`:

```axis
POLICY reliable_public_writes
  APPLIES_TO FLOW ALL
    METHOD IN post put patch delete
    NOT PATH STARTS_WITH "/internal"
  REQUIRE AUTH
  REQUIRE IDEMPOTENCY
```

`ALL` means every selector must match. `ANY` means at least one selector must match. An empty selector list matches every flow. `NOT` negates the selector immediately following it.

## Selectors

| Selector | Matches |
|---|---|
| `METHOD IN <methods...>` | HTTP method is in the list. |
| `READS <source>` | The flow reads the source, including inside nested MATCH, EACH, or TRY steps and nested expressions. |
| `WRITES <source>` | The flow inserts, upserts, updates, deletes, or fans out to the source, including nested steps. |
| `PATH STARTS_WITH "<prefix>"` | Route path begins with the prefix. |
| `NOT <selector>` | The nested selector does not match. |

Method names are written in lowercase, as in FLOW declarations.

## Requirements

Every requirement is enforced for every matched flow.

| Clause | Required flow property |
|---|---|
| `REQUIRE AUTH` | Any non-`none` AUTH declaration. |
| `REQUIRE AUTH ROLE <role>` | The specified role. |
| `REQUIRE LIMIT` | At least one LIMIT declaration. |
| `REQUIRE SCOPE` | A SCOPE declaration. |
| `REQUIRE RULE <name>` | A RULE step with that name. |
| `REQUIRE GUARD <name>` | A GUARD step with that name. |
| `REQUIRE IDEMPOTENCY` | An IDEMPOTENCY declaration. |
| `REQUIRE FANOUT` | At least one FANOUT operation, including in nested branches. |

## Messenger Example

```axis
POLICY idempotent_message_commands
  APPLIES_TO FLOW ALL
    METHOD IN post put patch delete
    WRITES messages
  REQUIRE AUTH
  REQUIRE IDEMPOTENCY

POLICY bulk_delivery_is_fanout
  APPLIES_TO FLOW ANY
    WRITES delivery_events
    WRITES sync_events
  REQUIRE FANOUT
```

The compiler evaluates policies after names, types, capabilities, indexes, and flow semantics are verified. A failure identifies the policy, flow, and exact missing clause, for example:

```text
ERROR: flow 'send_message' violates policy 'idempotent_message_commands': missing IDEMPOTENCY
```
