#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

set -a; eval "$(grep -v '^#' .env | grep -v '^\s*$' | sed 's/=\(.*\)/="\1"/')"; set +a

: "${DATABASE_URL:?Set DATABASE_URL in .env}"
: "${POSTGRES_USER:?Set POSTGRES_USER in .env}"
: "${POSTGRES_DB:?Set POSTGRES_DB in .env}"

CONTAINER="${COMPOSE_PROJECT_NAME:-helperbook-axis}-postgres-1"

run_psql() {
  if command -v psql &>/dev/null; then
    psql "$DATABASE_URL"
  else
    docker exec -i "$CONTAINER" psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"
  fi
}

echo "Generating migration SQL..."
cat src/*.axis | bin/axis --migrate /dev/stdin 2>/dev/null > sql/migrate.sql

if [[ ! -s sql/migrate.sql ]]; then
  echo "  -> no migrations to apply"
  exit 0
fi

echo "  -> sql/migrate.sql ($(wc -l < sql/migrate.sql) lines)"

if [[ "${1:-}" == "--apply" ]]; then
  echo "Applying migrations..."
  run_psql < sql/migrate.sql
  echo "  -> done"
else
  echo "  -> review sql/migrate.sql, then run: $0 --apply"
fi
