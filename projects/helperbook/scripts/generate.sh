#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p sql generated

echo "Generating artifacts from axis sources..."

cat src/*.axis | bin/axis --sql /dev/stdin 2>/dev/null > sql/schema.sql
cat src/*.axis | bin/axis --openapi /dev/stdin 2>/dev/null > generated/openapi.json
cat src/*.axis | bin/axis --ts /dev/stdin 2>/dev/null > generated/types.ts
cat src/*.axis | bin/axis --graphql /dev/stdin 2>/dev/null > generated/schema.graphql
cat src/*.axis | bin/axis --routes /dev/stdin 2>/dev/null > generated/routes.txt
cat src/*.axis | bin/axis --emit /dev/stdin 2>/dev/null > generated/plan.json
cat src/*.axis | bin/axis --testgen /dev/stdin 2>/dev/null > generated/tests.json
cat src/*.axis | bin/axis --observability /dev/stdin 2>/dev/null > generated/observability.yml
cat src/*.axis | bin/axis --deploy /dev/stdin 2>/dev/null > generated/deploy.txt

echo "Generated:"
wc -l sql/schema.sql generated/* | tail -1 | awk '{print "  " $1 " lines total"}'
ls -1 sql/schema.sql generated/* | while read f; do
  printf "  %-40s %s lines\n" "$f" "$(wc -l < "$f")"
done
