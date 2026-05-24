#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

set -a; eval "$(grep -v '^#' .env | grep -v '^\s*$' | sed 's/=\(.*\)/="\1"/')"; set +a

PORT="${PORT:-3000}"

echo "Verifying project..."
bin/axis --project . 2>&1
echo ""
echo "Starting axis server on port $PORT..."
bin/axis --serve .
