#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

set -a; eval "$(grep -v '^#' .env | grep -v '^\s*$' | sed 's/=\(.*\)/="\1"/')"; set +a

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

echo "Applying seed files..."
for f in sql/seeds/*.sql; do
  [ -f "$f" ] || continue
  echo "  -> $(basename "$f")"
  run_psql < "$f"
done
echo "  -> done"
