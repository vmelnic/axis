# SURFACE

Maps internal flows to versioned external API contracts. Decouples internal evolution from client compatibility.

## Syntax

```axis
SURFACE <name> <version>
  [REALM <realm_name>]
  [BASE_PATH <path>]

  ROUTE <method> <path> -> <flow_name>
  ...

  EXPOSE <ShapeName> AS <ExternalName>
    [FIELD <name> <type>]
    [HIDE <name>]
    [RENAME <name> AS <external_name>]
  ...

  [DEPRECATE <old_version> SUNSET <date>]
```

## Example

```axis
SURFACE public v1
  REALM booking_api
  BASE_PATH /api/v1

  ROUTE GET  /bookings       -> list_bookings
  ROUTE POST /bookings       -> create_booking
  ROUTE GET  /bookings/:id   -> get_booking

  EXPOSE Booking AS BookingResponse
    FIELD id         UUID
    FIELD status     STRING
    FIELD check_in   DATE
    FIELD check_out  DATE
    HIDE total_price
    HIDE user_id
    HIDE metadata

SURFACE public v2
  REALM booking_api
  BASE_PATH /api/v2

  ROUTE GET  /bookings       -> list_bookings_v2
  ROUTE POST /bookings       -> create_booking
  ROUTE GET  /bookings/:id   -> get_booking_v2

  EXPOSE Booking AS BookingResponse
    FIELD id          UUID
    FIELD status      STRING
    FIELD check_in    DATE
    FIELD check_out   DATE
    FIELD total_price DECIMAL
    FIELD guest_count INT
    RENAME note AS special_requests
    HIDE user_id
    HIDE metadata

  DEPRECATE v1 SUNSET "2027-06-01"
```

## BASE_PATH

`BASE_PATH <path>` prefixes all routes in the surface. Clients see `/api/v1/bookings`; the runtime dispatches to the `list_bookings` flow.

## ROUTE

`ROUTE <method> <path> -> <flow_name>` maps an external URL to an internal flow.

The route path is relative to BASE_PATH. The target flow must exist.

## EXPOSE

`EXPOSE <Shape> AS <ExternalName>` defines the external contract for a shape. Only listed fields are serialized in responses.

### FIELD

Declares a field in the external contract:

```axis
FIELD id UUID
FIELD status STRING
```

### HIDE

Explicitly excludes a field from the external response:

```axis
HIDE user_id
HIDE metadata
```

### RENAME

Maps an internal field name to an external one:

```axis
RENAME note AS special_requests
```

## DEPRECATE

`DEPRECATE <version> SUNSET <date>` marks a previous surface version as deprecated.

At runtime, the server adds `Deprecation` and `Sunset` headers to responses on the deprecated surface. After the sunset date, the runtime returns 410 Gone.

## Compiler Checks

- Surface realm must exist.
- Every ROUTE target flow must exist.
- Every EXPOSE shape must exist.
- EXPOSE FIELD fields must exist in the underlying shape with compatible types.
- HIDE and RENAME fields must exist in the shape.
