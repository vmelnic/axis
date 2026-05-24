# Language Overview

## Design Principles

1. **Context locality.** A single endpoint must be fully generatable within 512 Axis tokens. No cross-endpoint references.
2. **No implicit anything.** No hidden imports, ambient authority, global variables, or mutable shared state.
3. **Failure is syntax.** Unhandled errors, missing auth, unindexed queries -- these are parse or compile errors, not runtime surprises.
4. **The LLM describes; the runtime executes.** The LLM never writes SQL, retry logic, connection pools, or serialization code. It declares intent.
5. **One operation per binding.** No compound expressions. No operator precedence. Every `LET` binds exactly one operation.
6. **Endpoints cannot call endpoints.** Every flow is self-contained. Cross-service composition uses `SAGA`.

## Character Set

UTF-8. Keywords are ASCII uppercase. Identifiers are ASCII lowercase + digits + underscore. Shape names are PascalCase.

## Indentation

Significant whitespace: 2 spaces per level. Tabs are illegal. Trailing whitespace is ignored.

```axis
SHAPE User              -- level 0 (top-level construct)
  id UUID PK AUTO       -- level 1 (field)
  name STRING 100       -- level 1

FLOW get_user GET /users/:id    -- level 0
  AUTH session                  -- level 1 (declaration)
  LET user                      -- level 1 (step)
    FETCH users                  -- level 2 (expression)
      FILTER id EQ path.id       -- level 3 (clause)
    OR 404                       -- level 2
  RETURN 200 user                -- level 1
```

Indentation must increase by exactly 2 spaces. Decreasing indentation must land on a previously established level.

## Comments

Comments start with `--` and extend to end of line:

```axis
-- This is a comment
SHAPE User
  id UUID PK AUTO  -- inline comment
```

## Tokens

```
KEYWORD     = (see Reserved Keywords below)
IDENT       = [a-z_][a-z0-9_]*
SHAPE_NAME  = [A-Z][a-zA-Z0-9]*
PATH        = "/" [a-zA-Z0-9/:_\-.]+
INT_LIT     = "-"? [0-9]+
DECIMAL_LIT = "-"? [0-9]+ "." [0-9]+
STRING_LIT  = '"' [^"]* '"'
NEWLINE     = "\n"
INDENT      = increase in leading spaces (must be +2)
DEDENT      = decrease in leading spaces
COMMENT     = "--" [^\n]*
```

Identifiers are always lowercase with underscores. Shape names are PascalCase (start uppercase, contain at least one lowercase letter). Paths start with `/`.

## Reserved Keywords

### Construct Keywords

```
SHAPE SOURCE REALM FLOW SAGA SURFACE MIGRATE POLICY SERVICE STREAM FUNC STORAGE
```

### Database Type Keywords

```
POSTGRES MYSQL SQLITE REDIS ELASTICSEARCH DYNAMODB
```

### Flow Keywords

```
AUTH BODY PARAM HEADER RULE GUARD LET FETCH QUERY INSERT UPDATE DELETE
CALL EFFECT MATCH WHEN DEFAULT RETURN LIMIT CACHE SCOPE REQUIRE
SET EACH TRY RECOVER UPLOAD
```

### Expression Keywords

```
FILTER SORT ASC DESC CURSOR PAGE_SIZE OR AND NOT IF THEN ELSE
EQ NEQ GT GTE LT LTE IN BETWEEN LIKE EMPTY EXISTS
ADD SUB MUL DIV MOD ROUND CEIL FLOOR ABS
COUNT SUM AVG MIN MAX FIRST LAST
CONCAT LOWER UPPER TRIM SUBSTRING LENGTH STARTS_WITH ENDS_WITH CONTAINS
DAYS_BETWEEN HOURS_BETWEEN MINUTES_BETWEEN NOW NOW_PLUS NOW_MINUS FORMAT_DATE
COALESCE LITERAL TO_INT TO_DECIMAL TO_STRING
SELECT REDUCE SPLIT REPLACE FORMAT RENDER T
```

### Type Keywords

```
UUID STRING TEXT INT DECIMAL BOOL DATE TIMESTAMP ENUM REF LIST MAP JSON MAYBE BLOB
```

### Modifier Keywords

```
PK AUTO REQUIRED UNIQUE DEFAULT PRECISION SCALE MIN MAX
```

### Other Keywords

```
INDEX TENANT CAPABILITY FIELD HIDE EXPOSE DEPRECATE ROUTE
STEP VERIFY COMPENSATE YIELD ON_SUCCESS ON_FAILURE RUN_COMPENSATIONS
TEMPLATE DATA TASK APPLIES_TO WRITES READS METHOD WHERE
COPY COMPUTE DROP ANY NONE TRUE FALSE
HASH ASYNC WEBHOOK SIGNATURE HMAC
ITEMS TOTAL NEXT_CURSOR HAS_MORE
AS TO RENAME SUNSET BASE_PATH ENDPOINT VAULT INPUT OUTPUT
TIMEOUT RETRY BACKOFF TTL VARY WITH PARALLEL RECEIVE ON EVENT
BACKEND BUCKET PREFIX ACCESS MAX_SIZE TYPES MULTIPART
```

### Time Unit Keywords (lowercase)

```
seconds minutes hours days weeks months years
```

### Other Lowercase Keywords

```
read write call effect admin
per_second per_minute per_hour per_day per_user per_ip per_key global
session bearer api_key
exponential linear
email push_notification webhook
```

## Prefix-Only Expressions

All operators use prefix notation. The operator always comes first:

```axis
-- Correct (prefix)
EQ a b
ADD x y
MUL price quantity

-- Wrong (infix) -- NOT SUPPORTED
a == b
x + y
price * quantity
```

This eliminates operator precedence and makes parsing LL(1).

## LL(1) Grammar

Every parse decision is made from the current token with a single token of lookahead. No backtracking. This property enables constrained decoding: at any point during generation, the set of valid next tokens is deterministic.

## Program Structure

An Axis program is a sequence of top-level constructs. Order does not matter between constructs, but the compiler resolves all references after parsing.

```axis
-- Shapes first (data models)
SHAPE User
  ...

-- Sources (database bindings)
SOURCE users POSTGRES
  ...

-- Realms (permission scopes)
REALM api
  ...

-- Flows (API endpoints)
FLOW get_user GET /users/:id
  ...

-- Other constructs in any order
```

The 12 constructs are: [SHAPE](constructs/shape.md), [SOURCE](constructs/source.md), [REALM](constructs/realm.md), [FLOW](constructs/flow.md), [SAGA](constructs/saga.md), [SURFACE](constructs/surface.md), [POLICY](constructs/policy.md), [SERVICE](constructs/service.md), [MIGRATE](constructs/migrate.md), [STREAM](constructs/stream.md), [FUNC](constructs/func.md), [STORAGE](constructs/storage.md).

## Compilation Pipeline

```
Source text --> Lexer --> Tokens --> Parser --> AST --> Linker --> Linked AST
--> Verifier --> Verified AST --> Planner --> Execution Plan --> Codegen --> Target
```

### Lexer

Converts source text to token stream. Handles indentation tracking (INDENT/DEDENT tokens). Rejects tabs, non-2-space indentation, and unrecognized tokens.

### Parser

Builds AST from token stream. LL(1) -- parseable with a single token of lookahead. Rejects syntax errors.

### Linker

Resolves cross-references by content hash. Every shape name, source name, service name, and realm name is resolved. Rejects undefined references and structural cycles.

### Verifier

Type checking and semantic analysis. See [Verification Checks](#verification-checks) below.

### Planner

Optimization and execution planning. Detects N+1 patterns, merges redundant queries, selects optimal indexes, identifies parallel execution opportunities, and plans transactions.

### Codegen

Emits target artifacts: SQL DDL, Rust server, TypeScript types, OpenAPI spec, GraphQL schema, client SDKs, deployment manifests, migration plans, test suites, observability config.

## Verification Checks

The verifier catches errors at compile time:

**Type checking:**
- Every expression is type-checked. No implicit conversions.
- MAYBE types must be unwrapped via COALESCE before use in non-MAYBE context.

**Capability checking:**
- FETCH/QUERY requires `CAPABILITY read` in the flow's realm.
- INSERT/UPDATE/DELETE requires `CAPABILITY write`.
- CALL requires `CAPABILITY call`.
- EFFECT requires `CAPABILITY effect`.

**Index checking:**
- Every QUERY/FETCH must be covered by a declared INDEX. Unindexed access is a compile error with a suggested index.

**Totality checking:**
- Every FETCH must have an OR clause.
- Every UPDATE and DELETE must have an OR clause.
- Every CALL must have an OR clause.
- Every IF must have ELSE.
- Every MATCH must have DEFAULT.
- Every SAGA STEP must have COMPENSATE.

**Tenant checking:**
- If a realm declares TENANT, every flow accessing a tenanted source must declare SCOPE TENANT.
- SCOPE TENANT ANY requires admin capability.

**Policy checking:**
- All POLICY REQUIRE clauses are evaluated against every matching flow. Violations are compile errors.

**Ordering checking:**
- Flow operations must appear in the correct phase: declarations, validations, computations, mutations, effects, return.
- Effects cannot precede mutations. Mutations cannot precede guards.
- LET bindings are topologically ordered (no forward references).

**Surface checking:**
- Every EXPOSE field must exist in the underlying shape with a compatible type.
- Every ROUTE target flow must exist.
