#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

SVC="${1:-}"
if [[ -z "$SVC" ]]; then
  docker compose logs -f --tail 50
else
  docker compose logs -f --tail 50 "$SVC"
fi
