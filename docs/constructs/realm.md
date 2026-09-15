# REALM

Groups capabilities and tenant rules into a security domain. Every FLOW belongs to exactly one realm. The compiler uses realm declarations to verify that flows only access what they are authorized to use.

## Syntax

```axis
REALM <name>
  [TENANT <field_name>]
  CAPABILITY <kind> <target>
  ...
```

## Example

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

## TENANT

`TENANT <field_name>` declares the row-level isolation field for this realm. When declared, the compiler requires every flow in the realm that accesses tenanted sources to include a `SCOPE TENANT` declaration.

At runtime, `SCOPE TENANT auth.user_id` causes the server to automatically inject `WHERE <tenant_field> = <value>` into every QUERY and FETCH on tenanted sources within the flow.

```axis
REALM api
  TENANT user_id     -- all data access scoped by user_id
```

## CAPABILITY

Capability declarations whitelist what operations flows in this realm can perform. The compiler rejects any operation that lacks a matching capability.

### Capability Kinds

| Kind | Operations Enabled | Target |
|------|-------------------|--------|
| `read` | FETCH, QUERY | source name |
| `write` | INSERT, UPDATE, DELETE | source name |
| `call` | CALL | service name |
| `effect` | EFFECT | effect type (email, push_notification, webhook, async) |
| `admin` | SCOPE TENANT ANY (bypass tenant filtering) | source name |

### Examples

```axis
CAPABILITY read users          -- allows FETCH/QUERY on users source
CAPABILITY write bookings      -- allows INSERT/UPSERT/UPDATE/DELETE/FANOUT on bookings source
CAPABILITY call payments       -- allows CALL payments.* methods
CAPABILITY effect email        -- allows EFFECT email
CAPABILITY admin users         -- allows SCOPE TENANT ANY on users source
```

## Compiler Checks

- Realm names must not conflict.
- Every FETCH/QUERY on source S in a flow requires `CAPABILITY read S` in the flow's realm.
- Every INSERT/UPSERT/UPDATE/DELETE/FANOUT on source S requires `CAPABILITY write S`.
- Every CALL to service S requires `CAPABILITY call S`.
- Every EFFECT of type T requires `CAPABILITY effect T`.
- `SCOPE TENANT ANY` requires `CAPABILITY admin` in the realm.
- If the realm declares TENANT, every flow that accesses a tenanted source must declare SCOPE TENANT (enforceable via [POLICY](policy.md)).

## Usage in Flows

Flows declare their realm with the `REALM` keyword:

```axis
FLOW get_user GET /users/:id
  REALM booking_api        -- this flow operates under booking_api realm
  AUTH session
  SCOPE TENANT auth.user_id
  ...
```
