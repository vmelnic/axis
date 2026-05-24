#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

set -a; eval "$(grep -v '^#' .env | grep -v '^\s*$' | sed 's/=\(.*\)/="\1"/')"; set +a

: "${DATABASE_URL:?Set DATABASE_URL in .env}"
: "${POSTGRES_USER:?Set POSTGRES_USER in .env}"
: "${POSTGRES_DB:?Set POSTGRES_DB in .env}"

CONTAINER="${COMPOSE_PROJECT_NAME:-helperbook-axis}-postgres-1"

echo "Generating schema from axis sources..."
cat src/*.axis | bin/axis --sql /dev/stdin 2>/dev/null > sql/schema.sql
echo "  -> sql/schema.sql ($(wc -l < sql/schema.sql) lines)"

run_psql() {
  if command -v psql &>/dev/null; then
    psql "$DATABASE_URL"
  else
    docker exec -i "$CONTAINER" psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"
  fi
}

if [[ "${1:-}" == "--apply" ]]; then
  echo "Applying schema..."
  run_psql < sql/schema.sql
  echo "  -> done"
elif [[ "${1:-}" == "--reset" ]]; then
  echo "Resetting database and applying schema..."
  echo "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" | run_psql
  run_psql < sql/schema.sql
  echo "  -> done"
fi
