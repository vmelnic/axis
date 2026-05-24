#!/usr/bin/env bash
set -euo pipefail

PORT=${PORT:-3210}
BASE="http://localhost:$PORT"
PASS=0
FAIL=0
AXIS="../../target/release/axis"

check() {
  local name="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  PASS: $name"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $name (expected '$expected', got '$actual')"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== Starting infrastructure ==="
docker compose up -d --wait 2>/dev/null

echo "=== Building axis ==="
(cd ../.. && cargo build --release 2>/dev/null)

echo "=== Loading schema ==="
docker compose exec -T postgres psql -U axis -d axis -c "DROP TABLE IF EXISTS todos CASCADE;" >/dev/null
$AXIS --sql todolist.axis | docker compose exec -T postgres psql -U axis -d axis >/dev/null

echo "=== Starting server ==="
DATABASE_URL=postgres://axis:axis@localhost:5432/axis PORT=$PORT $AXIS --serve . &
SERVER_PID=$!
sleep 2

cleanup() {
  kill $SERVER_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Running tests ==="

# CREATE
echo "[CREATE]"
T1=$(curl -sf -X POST "$BASE/todos" -H "Content-Type: application/json" \
  -d '{"title":"Buy groceries","description":"Milk, eggs","priority":"high"}')
ID1=$(echo "$T1" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
check "create returns id" "true" "$([ -n "$ID1" ] && echo true || echo false)"
check "create status 201" "201" "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/todos" -H "Content-Type: application/json" -d '{"title":"Walk the dog"}')"
check "create with defaults" "false" "$(echo "$T1" | python3 -c "import sys,json; print(str(json.load(sys.stdin)['completed']).lower())")"

T3=$(curl -sf -X POST "$BASE/todos" -H "Content-Type: application/json" \
  -d '{"title":"Write tests","description":"For Axis","priority":"low"}')
ID3=$(echo "$T3" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

# LIST
echo "[LIST]"
check "list all" "3" "$(curl -sf "$BASE/todos" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"
check "list completed=false" "3" "$(curl -sf "$BASE/todos?completed=false" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"
check "list completed=true" "0" "$(curl -sf "$BASE/todos?completed=true" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"
check "list has pagination" "true" "$(curl -sf "$BASE/todos" | python3 -c "import sys,json; d=json.load(sys.stdin); print('true' if 'page' in d and 'page_size' in d else 'false')")"

# GET
echo "[GET]"
check "get by id" "Buy groceries" "$(curl -sf "$BASE/todos/$ID1" | python3 -c "import sys,json; print(json.load(sys.stdin)['title'])")"
check "get 404" "404" "$(curl -so /dev/null -w '%{http_code}' "$BASE/todos/00000000-0000-0000-0000-000000000000")"

# UPDATE
echo "[UPDATE]"
UPD=$(curl -sf -X PUT "$BASE/todos/$ID1" -H "Content-Type: application/json" -d '{"completed":true}')
check "update completed" "True" "$(echo "$UPD" | python3 -c "import sys,json; print(json.load(sys.stdin)['completed'])")"
check "update preserves title" "Buy groceries" "$(echo "$UPD" | python3 -c "import sys,json; print(json.load(sys.stdin)['title'])")"
check "update preserves description" "Milk, eggs" "$(echo "$UPD" | python3 -c "import sys,json; print(json.load(sys.stdin)['description'])")"
check "update preserves priority" "high" "$(echo "$UPD" | python3 -c "import sys,json; print(json.load(sys.stdin)['priority'])")"
check "update 404" "404" "$(curl -so /dev/null -w '%{http_code}' -X PUT "$BASE/todos/00000000-0000-0000-0000-000000000000" -H "Content-Type: application/json" -d '{"completed":true}')"
check "list completed=true after update" "1" "$(curl -sf "$BASE/todos?completed=true" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"

# DELETE
echo "[DELETE]"
check "delete 204" "204" "$(curl -so /dev/null -w '%{http_code}' -X DELETE "$BASE/todos/$ID3")"
check "delete removes item" "404" "$(curl -so /dev/null -w '%{http_code}' "$BASE/todos/$ID3")"
check "delete 404" "404" "$(curl -so /dev/null -w '%{http_code}' -X DELETE "$BASE/todos/00000000-0000-0000-0000-000000000000")"
check "count after delete" "2" "$(curl -sf "$BASE/todos" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"

# METRICS
echo "[METRICS]"
check "metrics endpoint" "true" "$(curl -sf "$BASE/metrics" | grep -q axis_request && echo true || echo false)"

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ] && exit 0 || exit 1
