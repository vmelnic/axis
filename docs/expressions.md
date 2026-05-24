# Expressions Reference

All expressions use prefix notation: the operator comes first.

```axis
-- Prefix (correct)
ADD a b
EQ x y
MUL price quantity

-- Infix (NOT supported)
a + b
x == y
```

## Arithmetic

| Op | Signature | Notes |
|----|-----------|-------|
| `ADD a b` | `(Num, Num) -> Num` | Same numeric type required |
| `SUB a b` | `(Num, Num) -> Num` | |
| `MUL a b` | `(Num, Num) -> Num` | `MUL INT DECIMAL` is a compile error |
| `DIV a b` | `(Num, Num) -> Num` | INT division truncates. Use `TO_DECIMAL` for precise division |
| `MOD a b` | `(INT, INT) -> INT` | |
| `ROUND a scale` | `(DECIMAL, INT) -> DECIMAL` | Round to N decimal places |
| `CEIL a` | `(DECIMAL) -> INT` | Ceiling |
| `FLOOR a` | `(DECIMAL) -> INT` | Floor |
| `ABS a` | `(Num) -> Num` | Absolute value |

## Comparison

| Op | Signature | Notes |
|----|-----------|-------|
| `EQ a b` | `(T, T) -> BOOL` | Same type required |
| `NEQ a b` | `(T, T) -> BOOL` | |
| `GT a b` | `(Ord, Ord) -> BOOL` | Ord = INT, DECIMAL, DATE, TIMESTAMP, STRING |
| `GTE a b` | `(Ord, Ord) -> BOOL` | |
| `LT a b` | `(Ord, Ord) -> BOOL` | |
| `LTE a b` | `(Ord, Ord) -> BOOL` | |
| `IN a vals...` | `(T, T...) -> BOOL` | `IN status pending confirmed` |
| `BETWEEN a lo hi` | `(Ord, Ord, Ord) -> BOOL` | Inclusive both ends |

## Boolean

| Op | Signature |
|----|-----------|
| `AND a b` | `(BOOL, BOOL) -> BOOL` |
| `OR a b` | `(BOOL, BOOL) -> BOOL` |
| `NOT a` | `(BOOL) -> BOOL` |

Multi-argument form with indentation:

```axis
LET eligible
  AND
    auth.email_verified
    EQ auth.account_status active
    GTE auth.trust_score 50
```

Indented AND/OR takes 2+ arguments (all must be BOOL).

## String

| Op | Signature | Example |
|----|-----------|---------|
| `CONCAT a b...` | `(STRING...) -> STRING` | `CONCAT "Hello " user.name` |
| `LOWER a` | `(STRING) -> STRING` | `LOWER user.email` |
| `UPPER a` | `(STRING) -> STRING` | `UPPER code` |
| `TRIM a` | `(STRING) -> STRING` | `TRIM input` |
| `SUBSTRING a start len` | `(STRING, INT, INT) -> STRING` | `SUBSTRING name 0 10` |
| `LENGTH a` | `(STRING) -> INT` | `LENGTH description` |
| `STARTS_WITH a prefix` | `(STRING, STRING) -> BOOL` | `STARTS_WITH url "https"` |
| `ENDS_WITH a suffix` | `(STRING, STRING) -> BOOL` | `ENDS_WITH email ".com"` |
| `CONTAINS a substr` | `(STRING, STRING) -> BOOL` | `CONTAINS bio "developer"` |
| `SPLIT a delimiter` | `(STRING, STRING) -> LIST STRING` | `SPLIT tags ","` |
| `REPLACE a from to` | `(STRING, STRING, STRING) -> STRING` | `REPLACE text "old" "new"` |

## Date / Time

| Op | Signature | Example |
|----|-----------|---------|
| `NOW` | `() -> TIMESTAMP` | `NOW` |
| `NOW_PLUS n unit` | `(INT, Unit) -> TIMESTAMP` | `NOW_PLUS 7 days` |
| `NOW_MINUS n unit` | `(INT, Unit) -> TIMESTAMP` | `NOW_MINUS 30 days` |
| `DAYS_BETWEEN a b` | `(DATE, DATE) -> INT` | `DAYS_BETWEEN check_in check_out` |
| `HOURS_BETWEEN a b` | `(TIMESTAMP, TIMESTAMP) -> INT` | `HOURS_BETWEEN NOW booking.start` |
| `MINUTES_BETWEEN a b` | `(TIMESTAMP, TIMESTAMP) -> INT` | |
| `FORMAT_DATE a fmt` | `(DATE/TIMESTAMP, STRING) -> STRING` | `FORMAT_DATE created_at "YYYY-MM-DD"` |

Time units: `seconds`, `minutes`, `hours`, `days`, `weeks`, `months`, `years`.

## Aggregate

Aggregates operate on QUERY results or bindings of type LIST.

| Op | Signature | Example |
|----|-----------|---------|
| `COUNT list` | `(LIST) -> INT` | `COUNT bookings` |
| `SUM list field` | `(LIST, Field) -> Num` | `SUM bookings total_price` |
| `AVG list field` | `(LIST, Field) -> DECIMAL` | `AVG reviews rating` |
| `MIN list field` | `(LIST, Field) -> T` | `MIN bookings check_in` |
| `MAX list field` | `(LIST, Field) -> T` | `MAX bookings check_out` |
| `FIRST list` | `(LIST) -> MAYBE T` | `FIRST sorted_listings` |
| `LAST list` | `(LIST) -> MAYBE T` | `LAST sorted_listings` |

## Control Flow

| Op | Syntax | Notes |
|----|--------|-------|
| `IF cond THEN a ELSE b` | `(BOOL, T, T) -> T` | Both branches must return same type. ELSE is required. |
| `COALESCE a default` | `(MAYBE T, T) -> T` | Unwrap optional with fallback. |
| `EMPTY list` | `(LIST) -> BOOL` | True if list has zero elements. |
| `EXISTS list` | `(LIST) -> BOOL` | True if list has one or more elements. |

```axis
LET display_name
  COALESCE user.nickname user.name

LET total
  IF has_discount
    THEN MUL base 0.90
    ELSE base
```

## Type Conversion

| Op | Signature |
|----|-----------|
| `TO_INT a` | `(DECIMAL/STRING) -> INT` |
| `TO_DECIMAL a` | `(INT/STRING) -> DECIMAL` |
| `TO_STRING a` | `(INT/DECIMAL/BOOL/DATE/TIMESTAMP/UUID) -> STRING` |
| `LITERAL value` | `() -> inferred type` |

`LITERAL` creates a typed literal value:

```axis
LET zero
  LITERAL 0
LET empty_string
  LITERAL ""
```

## Collection Operations

| Op | Syntax | Description |
|----|--------|-------------|
| `SELECT source fields...` | `(LIST, Field...) -> LIST` | Project specific fields from a list (map) |
| `FILTER source condition` | `(LIST, BOOL) -> LIST` | Filter a list by condition |
| `REDUCE op source field` | `(AggOp, LIST, Field) -> Num` | Reduce a list to a single value |

```axis
LET names
  SELECT users name email

LET active_users
  FILTER users EQ status active
```

## Template and i18n

| Op | Syntax | Description |
|----|--------|-------------|
| `RENDER template vars...` | `(STRING, ...) -> STRING` | Render a template with variable substitution |
| `T key vars...` | `(STRING, ...) -> STRING` | Translate a key using the current locale |
| `FORMAT template args...` | `(STRING, ...) -> STRING` | Format a string with positional arguments |

```axis
LET welcome_message
  RENDER welcome_email
    name user.name
    date booking.check_in

LET greeting
  T "greeting.hello"
    name user.name
```

Templates use `{{key}}` for variable substitution. Translation keys support dot notation for nested lookups.

## Literals

| Literal | Type | Example |
|---------|------|---------|
| Integer | INT | `42`, `-1`, `0` |
| Decimal | DECIMAL | `99.95`, `0.12` |
| String | STRING | `"hello"`, `"2026-05-08"` |
| Boolean | BOOL | `TRUE`, `FALSE` |
| None | MAYBE T | `NONE` |
| Now | TIMESTAMP | `NOW` |
| Identifier | Inferred | `pending`, `active` (bare enum variants) |

## Dot Paths

Dot paths access nested fields:

```axis
auth.user_id
body.listing_id
booking.listing.title
path.id
query.page
header.idempotency_key
```

Segments are separated by `.`. Each segment is a lowercase identifier.
