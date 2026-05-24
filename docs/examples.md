# Examples

## Booking Platform

A complete booking platform with users, listings, and bookings. From `examples/booking.axis`.

### Data Models

```axis
SHAPE User
  id UUID PK AUTO
  email STRING 255 REQUIRED UNIQUE
  name STRING 100 REQUIRED
  host_status ENUM active inactive DEFAULT inactive
  email_verified BOOL DEFAULT FALSE
  account_status ENUM active suspended banned DEFAULT active
  created_at TIMESTAMP AUTO

SHAPE Listing
  id UUID PK AUTO
  host_id UUID REF User.id REQUIRED
  title STRING 200 REQUIRED
  price_per_night DECIMAL PRECISION 10 SCALE 2 REQUIRED
  max_guests INT MIN 1 MAX 16 REQUIRED
  weekly_discount BOOL DEFAULT FALSE
  status ENUM active paused deleted REQUIRED
  created_at TIMESTAMP AUTO

SHAPE Booking
  id UUID PK AUTO
  user_id UUID REF User.id REQUIRED
  listing_id UUID REF Listing.id REQUIRED
  check_in DATE REQUIRED
  check_out DATE REQUIRED
  status ENUM pending confirmed cancelled completed REQUIRED
  total_price DECIMAL PRECISION 10 SCALE 2 REQUIRED
  guest_count INT MIN 1 MAX 16 DEFAULT 1
  created_at TIMESTAMP AUTO
  updated_at TIMESTAMP AUTO
```

### Database Sources and Security

```axis
SOURCE users POSTGRES
  SHAPE User
  INDEX email UNIQUE
  INDEX id

SOURCE listings POSTGRES
  SHAPE Listing
  INDEX host_id
  INDEX status

SOURCE bookings POSTGRES
  SHAPE Booking
  INDEX user_id created_at DESC
  INDEX listing_id check_in check_out
  INDEX status

REALM booking_api
  TENANT user_id
  CAPABILITY read users
  CAPABILITY read listings
  CAPABILITY read bookings
  CAPABILITY write bookings
  CAPABILITY effect email
```

### Simple Read Endpoint

A GET endpoint with auth, tenant scoping, and ownership guard:

```axis
FLOW get_booking get /bookings/:id
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  GUARD ownership 403 "not your booking"
    EQ booking.user_id auth.user_id
  RETURN 200 booking
```

### List with Pagination

A paginated list endpoint with optional status filter:

```axis
FLOW list_bookings get /bookings
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  PARAM page_size INT DEFAULT 20 MIN 1 MAX 100
  PARAM status MAYBE ENUM pending confirmed cancelled completed
  LET bookings
    QUERY bookings
      FILTER user_id EQ auth.user_id
      SORT created_at DESC
      PAGE_SIZE query.page_size
  RETURN 200 bookings
```

### Complex Write Endpoint

A POST endpoint with validation rules, guards, price calculation, and effects:

```axis
FLOW create_booking post /bookings
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  LIMIT 5 per_minute per_user
  BODY BookingCreate
    listing_id UUID REQUIRED
    check_in DATE REQUIRED
    check_out DATE REQUIRED
    guest_count INT MIN 1 MAX 16 DEFAULT 1

  RULE user_may_book
    REQUIRE auth.email_verified EQ TRUE
    REQUIRE auth.account_status EQ active

  GUARD valid_dates 400 "check_out must be after check_in"
    GT body.check_out body.check_in

  GUARD no_overlap 409 "dates are unavailable"
    EMPTY
      QUERY bookings
        FILTER listing_id EQ body.listing_id
        FILTER status IN pending
        FILTER check_in LT body.check_out
        FILTER check_out GT body.check_in

  LET listing
    FETCH listings
      FILTER id EQ body.listing_id
    OR 404 "listing not found"

  GUARD host_active 400 "host is not active"
    EQ listing.host_status active

  GUARD guest_capacity 400 "exceeds max guests"
    LTE body.guest_count listing.max_guests

  LET nights
    DAYS_BETWEEN body.check_in body.check_out
  LET nights_decimal
    TO_DECIMAL nights
  LET total
    MUL listing.price_per_night nights_decimal

  INSERT bookings
    user_id auth.user_id
    listing_id body.listing_id
    check_in body.check_in
    check_out body.check_out
    status pending
    total_price total
    guest_count body.guest_count
  AS booking

  EFFECT email
    TEMPLATE booking_request
    TO listing.host_id
    DATA booking

  RETURN 201 booking
```

Key patterns demonstrated:
- **RULE** for reusable authorization (email verified, account active)
- **GUARD** for inline validation (date ordering, availability, capacity)
- **Type conversion** with `TO_DECIMAL` before multiplying with DECIMAL
- **INSERT with AS** to bind the inserted row
- **EFFECT** for async email notification
- **Ordering**: declarations -> rules -> guards -> computations -> mutations -> effects -> return

## Project Structure Example

For larger applications, split across files. The `projects/helperbook/` project demonstrates this:

```
helperbook/
  src/
    auth.axis         # Auth shapes, sources, realms, flows
    messaging.axis    # Messaging system
    scheduling.axis   # Appointment scheduling
    commerce.axis     # Payments and transactions
    reputation.axis   # Reviews and ratings
    network.axis      # User connections
    admin.axis        # Admin panel
    settings.axis     # User settings
    policies.axis     # Cross-cutting policies
    finance.axis      # Financial reporting
    storage.axis      # File storage definitions
  templates/          # Email/notification templates
  locales/            # i18n translation files
  adapters/           # Service adapter configurations
```

Compile and verify: `axis --project helperbook/`

Serve: `axis --serve helperbook/`

## Patterns

### Public vs Protected Endpoints

```axis
FLOW public_listing get /listings/:id
  REALM api
  AUTH none
  LET listing
    FETCH listings
      FILTER id EQ path.id
      FILTER status EQ active
    OR 404
  RETURN 200 listing

FLOW my_listings get /my/listings
  REALM api
  AUTH session
  SCOPE TENANT auth.user_id
  LET listings
    QUERY listings
      FILTER host_id EQ auth.user_id
      SORT created_at DESC
  RETURN 200 listings
```

### Admin Endpoint with SCOPE TENANT ANY

```axis
FLOW admin_get_user get /admin/users/:id
  REALM admin_api
  AUTH role admin
  SCOPE TENANT ANY
  LET user
    FETCH users
      FILTER id EQ path.id
    OR 404
  RETURN 200 user
```

### Conditional Logic with MATCH

```axis
FLOW cancel_booking delete /bookings/:id
  REALM booking_api
  AUTH session
  SCOPE TENANT auth.user_id
  LET booking
    FETCH bookings
      FILTER id EQ path.id
    OR 404
  LET hours_until
    HOURS_BETWEEN NOW booking.check_in
  MATCH
    WHEN GTE hours_until 48
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
        SET refund_amount booking.total_price
      AS updated
      OR 500
    WHEN GTE hours_until 24
      LET half_refund
        MUL booking.total_price 0.50
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
        SET refund_amount half_refund
      AS updated
      OR 500
    DEFAULT
      UPDATE bookings
        WHERE id EQ path.id
        SET status cancelled
        SET refund_amount 0
      AS updated
      OR 500
  RETURN 200 updated
```

### Webhook Receiver

```axis
FLOW stripe_webhook WEBHOOK /webhooks/stripe
  REALM api
  AUTH WEBHOOK SIGNATURE stripe_webhook_secret HMAC sha256
  BODY StripeEvent
    type STRING REQUIRED
    data JSON REQUIRED
  MATCH
    WHEN EQ body.type "payment_intent.succeeded"
      UPDATE payments
        WHERE stripe_id EQ body.data.payment_intent_id
        SET status completed
      AS payment
      OR 404
    DEFAULT
      RETURN 200
  RETURN 200
```
