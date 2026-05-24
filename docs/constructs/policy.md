# POLICY

Compile-time invariants enforced across all matching flows. Policies turn "did you forget X?" from a semantic bug into a compile error.

## Syntax

```axis
POLICY <name>
  APPLIES_TO FLOW [WHERE <filter>]
  REQUIRE <clause>
  ...
```

## Examples

```axis
POLICY require_auth
  APPLIES_TO FLOW
  REQUIRE AUTH

POLICY require_rate_limit_on_writes
  APPLIES_TO FLOW
    WHERE METHOD IN POST PUT PATCH DELETE
  REQUIRE LIMIT

POLICY require_fraud_check_on_bookings
  APPLIES_TO FLOW
    WHERE WRITES bookings
  REQUIRE RULE fraud_score_ok

POLICY require_tenant_scope
  APPLIES_TO FLOW
    WHERE READS bookings
  REQUIRE SCOPE
```

## APPLIES_TO Selectors

Selectors determine which flows a policy applies to.

| Selector | Description | Example |
|----------|-------------|---------|
| `FLOW` | All flows | `APPLIES_TO FLOW` |
| `FLOW WHERE METHOD IN ...` | Flows with matching HTTP methods | `WHERE METHOD IN POST PUT DELETE` |
| `FLOW WHERE READS <source>` | Flows that FETCH or QUERY the source | `WHERE READS bookings` |
| `FLOW WHERE WRITES <source>` | Flows that INSERT, UPDATE, or DELETE the source | `WHERE WRITES bookings` |
| `FLOW WHERE PATH STARTS_WITH <path>` | Flows with matching URL path prefix | `WHERE PATH STARTS_WITH "/admin"` |

Multiple WHERE clauses narrow the scope (AND logic).

## REQUIRE Clauses

Each REQUIRE clause specifies something the matching flows must have.

| Clause | Description |
|--------|-------------|
| `REQUIRE AUTH` | Flow must have an AUTH declaration (any type). |
| `REQUIRE AUTH ROLE <role>` | Flow must require the specific role. |
| `REQUIRE LIMIT` | Flow must have at least one LIMIT declaration. |
| `REQUIRE SCOPE` | Flow must have a SCOPE TENANT declaration. |
| `REQUIRE RULE <name>` | Flow must include the named RULE step. |
| `REQUIRE GUARD <name>` | Flow must include the named GUARD step. |

Multiple REQUIRE clauses in a policy are all enforced (AND logic).

## Compiler Behavior

The compiler evaluates all policies after verification. For each policy:

1. Determine which flows match the APPLIES_TO selector.
2. For each matching flow, check all REQUIRE clauses.
3. If any clause is not satisfied, emit a compile error listing the policy name, the flow name, and what is missing.

Policy violations look like:

```
ERROR: flow 'create_booking' violates policy 'require_rate_limit_on_writes': missing LIMIT
```

## Combining Policies

Policies compose naturally. Multiple policies can target the same flows:

```axis
-- All flows must have auth
POLICY require_auth
  APPLIES_TO FLOW
  REQUIRE AUTH

-- Write flows must have rate limiting
POLICY require_rate_limit_on_writes
  APPLIES_TO FLOW
    WHERE METHOD IN POST PUT PATCH DELETE
  REQUIRE LIMIT

-- Flows reading user data must have tenant scope
POLICY require_tenant_scope
  APPLIES_TO FLOW
    WHERE READS users
  REQUIRE SCOPE
```

A write flow reading users would need to satisfy all three.
