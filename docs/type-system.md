# Type System

## Primitive Types

| Type | Description | Axis Literal | SQL (Postgres) | SQL (MySQL) | SQL (SQLite) |
|------|-------------|-------------|----------------|-------------|--------------|
| `UUID` | 128-bit identifier | `"550e8400-..."` | `UUID` | `CHAR(36)` | `TEXT` |
| `STRING N` | UTF-8 string, max N bytes | `"hello"` | `VARCHAR(N)` | `VARCHAR(N)` | `TEXT` |
| `TEXT` | Unbounded UTF-8 string | `"long text..."` | `TEXT` | `TEXT` | `TEXT` |
| `INT` | 64-bit signed integer | `42`, `-1` | `BIGINT` | `BIGINT` | `INTEGER` |
| `DECIMAL` | Fixed-point number | `99.95` | `NUMERIC(P,S)` | `DECIMAL(P,S)` | `REAL` |
| `BOOL` | Boolean | `TRUE`, `FALSE` | `BOOLEAN` | `BOOLEAN` | `INTEGER` |
| `DATE` | Calendar date (no time) | `"2026-05-08"` | `DATE` | `DATE` | `TEXT` |
| `TIMESTAMP` | UTC datetime, microsecond | `"2026-05-08T14:30:00Z"` | `TIMESTAMPTZ` | `DATETIME` | `TEXT` |
| `BLOB` | Binary data | -- | `BYTEA` | `LONGBLOB` | `BLOB` |

## Compound Types

| Type | Description | Example |
|------|-------------|---------|
| `ENUM v1 v2 ...` | Sum of named variants | `ENUM pending confirmed cancelled` |
| `REF Shape.field` | Foreign key reference | `REF User.id` |
| `LIST T` | Ordered collection of type T | `LIST STRING` |
| `MAP K V` | Key-value pairs | `MAP STRING INT` |
| `JSON` | Opaque JSON value | -- |
| `MAYBE T` | Optional value of type T | `MAYBE TEXT` |

### ENUM

Named variants. Variants are lowercase identifiers. At least one variant is required.

```axis
status ENUM pending confirmed cancelled completed REQUIRED
```

Enum values are used as bare identifiers in expressions:

```axis
SET status confirmed
```

### REF

Foreign key. The compiler verifies the target shape and field exist.

```axis
user_id UUID REF User.id REQUIRED
```

### LIST and MAP

Generic collection types:

```axis
tags LIST STRING
metadata MAP STRING INT
```

MAP keys must be `STRING`, `UUID`, or `INT`.

### MAYBE

Optional value. Fields without MAYBE and without DEFAULT are implicitly required.

```axis
note MAYBE TEXT
nickname MAYBE STRING 50
```

MAYBE values must be unwrapped with `COALESCE` before use in non-optional context:

```axis
LET display_name
  COALESCE user.nickname user.name
```

## Field Modifiers

| Modifier | Description | Applies To |
|----------|-------------|-----------|
| `PK` | Primary key. Exactly one per shape. | Any type |
| `AUTO` | Runtime-generated (UUID v7 for UUID, current timestamp for TIMESTAMP). Cannot appear in INSERT. | UUID, TIMESTAMP |
| `REQUIRED` | Must be present in INSERT. Compile error if omitted. | Any type |
| `UNIQUE` | Unique constraint enforced at runtime. | Any type |
| `DEFAULT value` | Default value if omitted in INSERT. | Any type |
| `MIN value` | Minimum bound. Compile-time and runtime validated. | INT, DECIMAL |
| `MAX value` | Maximum bound. Compile-time and runtime validated. | INT, DECIMAL |
| `PRECISION p` | Decimal precision (total digits). | DECIMAL |
| `SCALE s` | Decimal scale (digits after point). | DECIMAL |

Modifiers can be combined:

```axis
SHAPE Product
  id UUID PK AUTO
  name STRING 200 REQUIRED
  price DECIMAL PRECISION 10 SCALE 2 REQUIRED MIN 0
  stock INT DEFAULT 0 MIN 0
  sku STRING 50 REQUIRED UNIQUE
  description MAYBE TEXT
  created_at TIMESTAMP AUTO
  updated_at TIMESTAMP AUTO
```

## Type Rules

### No Implicit Conversions

Types do not auto-promote. `INT` does not convert to `DECIMAL`. Use explicit conversion:

```axis
LET price_decimal
  TO_DECIMAL quantity
```

### No Null

There is no null. Use `MAYBE T` for optional fields and `COALESCE` to unwrap with a default.

### Operation Return Types

| Operation | Returns |
|-----------|---------|
| `FETCH source ... OR code` | Shape type (total -- never MAYBE) |
| `QUERY source` | LIST of shape type (always succeeds, may be empty) |
| `INSERT source ... AS name` | Inserted row (shape type, includes AUTO fields) |
| `UPDATE source ... AS name OR code` | Updated row (shape type) |
| `UPDATE source ... COUNT name` | INT (number of affected rows) |
| `DELETE source ... OR code` | Nothing |
| `CALL service.method ... OR code` | Service method OUTPUT type |

### Arithmetic Rules

- `ADD`, `SUB`, `MUL`, `DIV`, `MOD` require both operands to be the same numeric type.
- Result is the same type as operands.
- `DIV` on `INT` is integer division. Use `TO_DECIMAL` for precise division.
- `MUL INT DECIMAL` is a compile error.

### Comparison Rules

- `EQ`, `NEQ`, `GT`, `GTE`, `LT`, `LTE` require same type on both sides.
- Return `BOOL`.

### Boolean Rules

- `AND`, `OR` require `BOOL` operands. Return `BOOL`.
- `NOT` requires `BOOL`. Returns `BOOL`.

## Automatic Bindings

These names are available without declaration within a FLOW:

| Name | Type | Available When |
|------|------|----------------|
| `auth` | JWT claims object | `AUTH` declared (not `none`) |
| `body` | Body shape | `BODY` declared |
| `path` | Record of path params | Path contains `:param` segments |
| `query` | Record of query params | `PARAM` declared |
| `header` | Record of declared headers | `HEADER` declared |

Access fields with dot notation: `auth.user_id`, `body.email`, `path.id`, `query.page`.
