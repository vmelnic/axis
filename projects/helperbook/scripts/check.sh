#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

set -a; eval "$(grep -v '^#' .env | grep -v '^\s*$' | sed 's/=\(.*\)/="\1"/')"; set +a

CONTAINER="${COMPOSE_PROJECT_NAME:-helperbook-axis}-postgres-1"
PORT="${PORT:-3000}"

echo "=== Infrastructure ==="
for svc in postgres redis meilisearch prometheus grafana mailcatcher; do
  name="${COMPOSE_PROJECT_NAME:-helperbook-axis}-${svc}-1"
  status=$(docker inspect -f '{{.State.Health.Status}}' "$name" 2>/dev/null || echo "$(docker inspect -f '{{.State.Status}}' "$name" 2>/dev/null || echo "not running")")
  printf "  %-15s %s\n" "$svc" "$status"
done

echo ""
echo "=== Axis ==="
if curl -sf "http://localhost:${PORT}/health" >/dev/null 2>&1; then
  echo "  server        healthy (port $PORT)"
else
  echo "  server        not running (port $PORT)"
fi

echo ""
echo "=== Project ==="
bin/axis --project . 2>&1

echo ""
echo "=== Database ==="
TABLE_COUNT=$(docker exec "$CONTAINER" psql -U "${POSTGRES_USER:-helperbook}" -d "${POSTGRES_DB:-helperbook}" -tAc "SELECT count(*) FROM information_schema.tables WHERE table_schema='public'" 2>/dev/null || echo "?")
echo "  tables        $TABLE_COUNT"
