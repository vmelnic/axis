# SHAPE

Declares a data structure. Shapes define the schema for database tables, request bodies, and response objects. Shapes are database-agnostic -- the same shape works with any backend.

## Syntax

```axis
SHAPE <ShapeName>
  <field_name> <type> [modifiers...]
  ...
```

## Example

```axis
SHAPE Booking
  id            UUID        PK AUTO
  user_id       UUID        REF User.id REQUIRED
  listing_id    UUID        REF Listing.id REQUIRED
  check_in      DATE        REQUIRED
  check_out     DATE        REQUIRED
  status        ENUM pending confirmed cancelled completed REQUIRED
  total_price   DECIMAL     PRECISION 10 SCALE 2 REQUIRED
  guest_count   INT         MIN 1 MAX 16 DEFAULT 1
  note          MAYBE TEXT
  metadata      MAYBE JSON
  created_at    TIMESTAMP   AUTO
  updated_at    TIMESTAMP   AUTO
```

## Fields

Each field has a name, a type, and zero or more modifiers.

Field names are lowercase identifiers: `[a-z_][a-z0-9_]*`.

Shape names are PascalCase: start with uppercase, contain at least one lowercase letter.

### Types

See [Type System](../type-system.md) for the full type reference.

### Modifiers

| Modifier | Description |
|----------|-------------|
| `PK` | Primary key. Exactly one per shape. |
| `AUTO` | Runtime-generated. UUID fields get v7 UUIDs; TIMESTAMP fields get current time. Cannot appear in INSERT values. |
| `REQUIRED` | Must be present when inserting. Compile error if INSERT omits it. |
| `UNIQUE` | Unique constraint. Runtime enforces via database constraint. |
| `DEFAULT value` | Default value if omitted in INSERT. Value can be an integer, decimal, string, boolean, identifier, or `NOW`. |
| `MIN value` | Minimum bound for INT or DECIMAL. Validated at compile time and runtime. |
| `MAX value` | Maximum bound for INT or DECIMAL. Validated at compile time and runtime. |
| `PRECISION p` | Total digits for DECIMAL. |
| `SCALE s` | Digits after decimal point for DECIMAL. |
| `REF Shape.field` | Foreign key reference. Compiler verifies target shape and field exist. Can also be specified as a type: `user_id UUID REF User.id`. |

## Compiler Checks

- Duplicate shape names are rejected.
- REF targets are verified: both the shape and the field must exist.
- ENUM types must have at least one variant.
- A warning is emitted if a shape has no PK field.
- All REQUIRED, AUTO, and DEFAULT modifiers are enforced when INSERT is used on a source bound to this shape.

## Relationship to SOURCE

A SHAPE defines the logical schema. A [SOURCE](source.md) maps a shape to a physical database. Operations (FETCH, QUERY, INSERT, UPDATE, DELETE) target sources, not shapes.

```axis
SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE

SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE
```
