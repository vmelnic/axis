# FUNC

Reusable pure function with typed inputs and output. Functions encapsulate computation that can be called from multiple flows.

## Syntax

```axis
FUNC <name>
  INPUT <param1> <type1>
  [INPUT <param2> <type2>]
  ...
  OUTPUT <type>
  [<flow_steps>...]
  RETURN <expr>
```

## Example

```axis
FUNC calculate_total
  INPUT price DECIMAL
  INPUT nights INT
  INPUT has_discount BOOL
  OUTPUT DECIMAL
  LET base
    MUL price TO_DECIMAL nights
  LET total
    IF has_discount
      THEN MUL base 0.90
      ELSE base
  LET service_fee
    ROUND
      MUL total 0.12
      2
  RETURN ADD total service_fee

FUNC full_name
  INPUT first STRING
  INPUT last STRING
  OUTPUT STRING
  RETURN CONCAT first " " last
```

## INPUT

Each INPUT declares a named, typed parameter. Multiple INPUT declarations are allowed.

## OUTPUT

Declares the return type of the function.

## Body

The function body can contain flow steps that don't perform I/O:

- `LET` -- variable binding
- `SET` -- update a binding
- `MATCH` -- conditional branching
- `EACH` -- iteration
- `TRY/RECOVER` -- error recovery
- `GUARD` -- validation

Functions can also contain `INSERT`, `UPDATE`, `DELETE`, and `EFFECT`, though these are more commonly found in flows.

## RETURN

The function must end with a RETURN expression whose type matches OUTPUT.

## Usage in Flows

Functions are called with positional arguments:

```axis
FLOW get_booking GET /bookings/:id
  REALM api
  AUTH session
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  LET listing
    FETCH listings
      FILTER id EQ booking.listing_id
    OR 500
  LET total
    calculate_total listing.price_per_night booking.nights booking.has_discount
  RETURN 200
    booking_id booking.id
    total total
```

Function calls in expressions are resolved by the parser when it encounters an identifier that matches a declared FUNC name.
