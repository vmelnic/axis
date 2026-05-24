# MIGRATE

Schema evolution. Transforms a shape from one version to another.

## Syntax

```axis
MIGRATE <ShapeName> <from_version> TO <to_version>
  [COPY <field1> [field2...]]
  [COMPUTE <field> <expr>]
  [DROP <field>]
  [ADD <field> <type> [modifiers...]]
  [RENAME <old_field> TO <new_field>]
```

## Example

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

## Operations

### COPY

Carry forward fields unchanged from the old version:

```axis
COPY id user_id email name created_at
```

Multiple fields can be listed on one line.

### COMPUTE

Derive a new field value from old fields using an expression:

```axis
COMPUTE total_price
  MUL v1.nights v1.price_per_night
```

The expression can reference old fields with `v1.<field>` notation and use any Axis expression operator.

### DROP

Remove a field:

```axis
DROP nights
DROP price_per_night
```

### ADD

Add a new field with type and optional modifiers. A DEFAULT is typically required since existing rows need a value:

```axis
ADD cancellation_policy ENUM flexible moderate strict DEFAULT flexible
ADD updated_at TIMESTAMP DEFAULT NOW
```

### RENAME

Rename a field without changing its data:

```axis
RENAME old_name TO new_name
```

## Code Generation

The `--migrate` CLI flag generates migration SQL from MIGRATE constructs:

```bash
axis --migrate app.axis
```

This produces up/down SQL files for each migration, including:
- ALTER TABLE statements for ADD, DROP, RENAME
- Data backfill for COMPUTE fields
- Rollback SQL for each operation

The `--migrate-runner` flag generates a standalone Rust migration runner project:

```bash
axis --migrate-runner app.axis
```

## Compiler Checks

- The shape referenced by MIGRATE must exist.
- COMPUTE expressions are validated for type correctness.
