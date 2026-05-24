# Helperbook

Telegram-like marketplace connecting clients with service providers. Full production backend written entirely in Axis — zero application source code.

Full production backend written entirely in Axis — zero application source code. Run `axis --project .` for current counts.

## Quick start

```bash
make up              # start postgres, redis, meilisearch, prometheus, grafana, mailcatcher
make schema-reset    # create all tables
make seed            # insert dev data (users, categories, app versions)
make serve           # start axis server on $PORT (default 3000)
```

## Project structure

```
src/                    Axis source files (the entire backend)
sql/
  schema.sql            Generated DDL (topologically sorted by FK deps)
  seeds/                Seed data files, applied in order by make seed
generated/              All generated artifacts (make generate)
  openapi.json          OpenAPI 3.0 spec
  types.ts              TypeScript type definitions
  schema.graphql        GraphQL schema
  routes.txt            Route listing
  plan.json             Execution plan
  tests.json            Generated test suite
  observability.yml     Prometheus/metrics config
  deploy.txt            Deployment manifests
adapters/               16 WASM service adapters (hot-reloadable)
templates/              Email and push notification templates
locales/                i18n translations (en, ro, ru)
grafana/                Provisioned datasource + Helperbook dashboard
scripts/                Utility scripts invoked by Makefile
bin/axis                Compiled axis binary
docker-compose.yml      Postgres, Redis, Meilisearch, Prometheus, Grafana, Mailcatcher
prometheus.yml          Scrape config for axis /metrics
.env                    Environment variables (all config lives here)
Makefile                All commands
```

## Makefile

```
make help             Show all commands
make up               Start all services
make down             Stop all services
make restart          Restart all services
make serve            Start axis HTTP server
make check            Check infrastructure, project, and database status
make schema           Generate SQL DDL to sql/schema.sql
make schema-apply     Generate and apply schema (idempotent)
make schema-reset     Drop all tables and recreate schema
make migrate          Generate migration SQL to sql/migrate.sql
make migrate-apply    Generate and apply migrations
make seed             Apply all sql/seeds/*.sql files in order
make generate         Regenerate all artifacts
make logs             Tail all service logs (or: make logs SVC=postgres)
```

## Source files

| File | Description |
|------|-------------|
| auth.axis | Phone/email OTP, social login (Google, Apple, LinkedIn, Meta), TOTP 2FA, phone/email verification, account deletion, session management, Stripe webhooks |
| network.axis | Service categories, provider services, contacts, favorites, notes, folders, connections, geo search (Nominatim/OSRM), provider discovery, search indexing |
| messaging.axis | Direct messages, group conversations, file attachments (UPLOAD), AI translate/summarize/suggest, message search (Meilisearch), edit/delete with time windows, read receipts, archive, WebSocket + SSE streams |
| scheduling.axis | Provider profiles, onboarding state machine, availability slots, status, appointments (propose/confirm/dismiss/cancel/start/complete/no-show), Google Calendar sync, real-time streams |
| commerce.axis | Stripe subscriptions, checkout, billing history, service history, referral codes with Plus week rewards |
| finance.axis | Provider earnings, commission tracking, payout methods (bank/PayPal/Stripe Connect), payout requests with balance guards |
| reputation.axis | Reviews with AI moderation, review responses, ID verification badges with document upload (UPLOAD), disputes with appointment guards, user reports |
| settings.axis | User preferences, notification settings, privacy controls, blocked users, push device tokens, avatar upload (UPLOAD), gallery with upload, email/push services, storage (S3), notification streams |
| admin.axis | Badge review, user moderation, reports, app versions, deep links, audit log, dashboard metrics, recent activity, health check, SURFACE with all routes |
| policies.axis | Computed functions: average_rating, is_verified_provider, provider_completion_rate |
| storage.axis | File storage definitions: avatars (public, 5MB), chat attachments (private, 10MB), badge documents (private, 20MB) |

## Services

| Service | Adapter | Methods |
|---------|---------|---------|
| sms | twilio | send_otp |
| google_auth | google-oauth | verify_token |
| apple_auth | apple-oauth | verify_token |
| linkedin_auth | linkedin-oauth | exchange_code, get_profile |
| meta_auth | meta-oauth | exchange_code, get_profile |
| totp | totp | generate_secret, verify_code |
| geo | nominatim | geocode, reverse_geocode, distance |
| search_providers | meilisearch | index, search, remove |
| openai | openai | moderate, translate, summarize, suggest_replies |
| search | meilisearch | search |
| calendar | google-calendar | create_event, update_event, delete_event, list_events |
| payment | stripe | create_checkout, verify_receipt |
| email | sendgrid/mailgun/smtp | send, send_otp, send_template |
| push | firebase-fcm | send_notification |
| storage | s3 | presign_upload, delete_object |
| moderation | openai | analyze |

Swap providers by changing `*_SERVICE_URL` in `.env` (e.g. `EMAIL_SERVICE_URL=adapter://mailgun`). Adapters hot-reload on config change.

## Observability

Prometheus scrapes axis at `/metrics` every 10s. Grafana auto-provisions at `localhost:3001` with a Helperbook dashboard covering:

- Request rate and latency (p50/p95/p99)
- Error rate and status code distribution
- Top and slowest endpoints
- DB query duration and connection pool stats
- Rate limit rejections
- Cache hit rate
- WebSocket/SSE active connections

## Configuration

All configuration is in `.env`. See `.env.example` for the full list. Key variables:

```
DATABASE_URL          Postgres connection string
JWT_SECRET            JWT signing secret
PORT                  Server port (default 3000)
POSTGRES_USER         DB user (used by scripts for psql)
POSTGRES_DB           DB name (used by scripts for psql)
```

Per-service URLs (`SMS_SERVICE_URL`, `EMAIL_SERVICE_URL`, etc.) and API keys (`STRIPE_SECRET_KEY`, `OPENAI_API_KEY`, etc.) are documented in `.env.example` and each adapter's `.env.example`.

## Logging

Configurable via two env vars:

```
AXIS_LOG_LEVEL=info           # Global log level: trace, debug, info, warn, error
AXIS_LOG_CHANNELS=all         # Comma-separated channels, or "all"
```

Available channels: `request`, `database`, `files`, `cache`, `auth`, `lifecycle`. Examples:

```bash
AXIS_LOG_CHANNELS=request,database    # Only HTTP requests and DB queries
AXIS_LOG_CHANNELS=all                 # Everything (default)
AXIS_LOG_LEVEL=debug AXIS_LOG_CHANNELS=auth,files  # Debug auth and file uploads
```

## File storage

STORAGE constructs define file upload destinations. Each storage specifies a backend (`local` or `s3`), bucket path, access level (`public` or `private`), max file size, and allowed file types.

Local public files are served at `/files/{bucket}/{filename}`. Private files return a filesystem path for use with signed URLs or access-controlled endpoints.

This project defines three storages: `avatars` (public, served at `/files/uploads/avatars/`), `chat_attachments` (private), and `badge_documents` (private).
