#!/usr/bin/env bash
set -euo pipefail

PORT=${PORT:-3211}
BASE="http://localhost:$PORT"
PASS=0
FAIL=0
AXIS="../../target/release/axis"
JWT_SECRET="advanced-test-secret"

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

make_jwt() {
  local user_id="$1" verified="$2" role="${3:-user}"
  python3 - <<PYEOF
import base64, json, hmac as _hmac, hashlib, time
def b64url(d):
    if isinstance(d, str): d = d.encode()
    return base64.urlsafe_b64encode(d).rstrip(b'=').decode()
header = b64url(json.dumps({"alg":"HS256","typ":"JWT"}, separators=(',',':')))
payload = b64url(json.dumps({
    "sub": "$user_id", "user_id": "$user_id",
    "verified": True if "$verified" == "true" else False, "role": "$role",
    "exp": int(time.time()) + 3600
}, separators=(',',':')))
msg = header + "." + payload
sig = b64url(_hmac.new("$JWT_SECRET".encode(), msg.encode(), hashlib.sha256).digest())
print(msg + "." + sig)
PYEOF
}

echo "=== Starting infrastructure ==="
docker compose up -d --wait 2>/dev/null

echo "=== Building axis ==="
(cd ../.. && cargo build --release 2>/dev/null)

echo "=== Loading schema ==="
docker compose exec -T postgres psql -U axis -d axis -c "DROP TABLE IF EXISTS expenses CASCADE;" >/dev/null
docker compose exec -T postgres psql -U axis -d axis -c "DROP TABLE IF EXISTS accounts CASCADE;" >/dev/null
$AXIS --sql advanced.axis | docker compose exec -T postgres psql -U axis -d axis >/dev/null

echo "=== Starting server ==="
DATABASE_URL=postgres://axis:axis@localhost:5433/axis JWT_SECRET="$JWT_SECRET" PORT=$PORT $AXIS --serve . &
SERVER_PID=$!
sleep 2

cleanup() { kill $SERVER_PID 2>/dev/null || true; }
trap cleanup EXIT

echo "=== Generating tokens ==="
TOKEN_A=$(make_jwt "user-a" "true" "user")
TOKEN_B=$(make_jwt "user-b" "true" "user")
TOKEN_C=$(make_jwt "user-c" "false" "viewer")

echo "=== Running tests ==="

# REGISTER
echo "[REGISTER]"
RA=$(curl -sf -X POST "$BASE/register" -H "Content-Type: application/json" \
  -d '{"email":"a@test.com","user_id":"user-a"}')
check "register user-a id" "true" \
  "$(echo "$RA" | python3 -c "import sys,json; print('true' if json.load(sys.stdin).get('id') else 'false')")"
check "register user-b 201" "201" \
  "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/register" \
     -H "Content-Type: application/json" -d '{"email":"b@test.com","user_id":"user-b"}')"
check "register user-c" "201" \
  "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/register" \
     -H "Content-Type: application/json" -d '{"email":"c@test.com","user_id":"user-c"}')"

docker compose exec -T postgres psql -U axis -d axis \
  -c "UPDATE accounts SET verified = true WHERE user_id IN ('user-a','user-b');" >/dev/null

# AUTH
echo "[AUTH]"
check "no token 401" "401" \
  "$(curl -so /dev/null -w '%{http_code}' "$BASE/me")"
check "bad token 401" "401" \
  "$(curl -so /dev/null -w '%{http_code}' "$BASE/me" -H "Authorization: Bearer bad.token")"
check "valid token 200" "200" \
  "$(curl -so /dev/null -w '%{http_code}' "$BASE/me" -H "Authorization: Bearer $TOKEN_A")"
check "get_me user_id" "user-a" \
  "$(curl -sf "$BASE/me" -H "Authorization: Bearer $TOKEN_A" \
     | python3 -c "import sys,json; print(json.load(sys.stdin)['user_id'])")"

# TENANT ISOLATION
echo "[TENANT]"
curl -sf -X POST "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
  -H "Content-Type: application/json" \
  -d '{"title":"Rent","amount":1200,"category":"misc"}' >/dev/null
check "user-a sees own" "1" \
  "$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
     | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"
check "user-b sees 0" "0" \
  "$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_B" \
     | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"

# RULE
echo "[RULE]"
check "unverified 403" "403" \
  "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/expenses" \
     -H "Authorization: Bearer $TOKEN_C" -H "Content-Type: application/json" \
     -d '{"title":"Coffee","amount":5,"category":"food"}')"

# GUARD
echo "[GUARD]"
check "GT: negative 400" "400" \
  "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/expenses" \
     -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
     -d '{"title":"Bad","amount":-10,"category":"food"}')"
check "LTE: excessive 400" "400" \
  "$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/expenses" \
     -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
     -d '{"title":"Big","amount":99999,"category":"food"}')"
EXP_A_ID=$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['items'][0]['id'])")
check "ownership tenant-isolated 404" "404" \
  "$(curl -so /dev/null -w '%{http_code}' "$BASE/expenses/$EXP_A_ID" \
     -H "Authorization: Bearer $TOKEN_B")"

# FUNC
echo "[FUNC]"
EXP=$(curl -sf -X POST "$BASE/expenses" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"title":"Laptop","amount":999,"category":"tech","note":"work"}')
EXP_ID=$(echo "$EXP" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
check "create with FUNC" "true" "$([ -n "$EXP_ID" ] && echo true || echo false)"

curl -sf -X POST "$BASE/expenses" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"title":"Chair","amount":299,"category":"misc"}' >/dev/null

# EXPR / EACH / INLINE RETURN
echo "[EXPR]"
SUM=$(curl -sf "$BASE/summary" -H "Authorization: Bearer $TOKEN_A")
check "summary user_id" "user-a" \
  "$(echo "$SUM" | python3 -c "import sys,json; print(json.load(sys.stdin)['user_id'])")"
check "summary role" "user" \
  "$(echo "$SUM" | python3 -c "import sys,json; print(json.load(sys.stdin)['role'])")"
check "EACH counter >= 3" "true" \
  "$(echo "$SUM" | python3 -c "import sys,json; print('true' if json.load(sys.stdin)['expense_count'] >= 3 else 'false')")"
check "LENGTH user-a = 6" "6" \
  "$(echo "$SUM" | python3 -c "import sys,json; print(json.load(sys.stdin)['uid_length'])")"

# MATCH
echo "[MATCH]"
CAT=$(curl -sf -X POST "$BASE/expenses/$EXP_ID/categorize" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"category":"travel"}')
check "WHEN travel" "travel" \
  "$(echo "$CAT" | python3 -c "import sys,json; print(json.load(sys.stdin)['category'])")"
CAT2=$(curl -sf -X POST "$BASE/expenses/$EXP_ID/categorize" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"category":"food"}')
check "WHEN food" "food" \
  "$(echo "$CAT2" | python3 -c "import sys,json; print(json.load(sys.stdin)['category'])")"
EXP3_ID=$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['items'][-1]['id'])")
CAT3=$(curl -sf -X POST "$BASE/expenses/$EXP3_ID/categorize" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"category":"misc"}')
check "DEFAULT misc" "misc" \
  "$(echo "$CAT3" | python3 -c "import sys,json; print(json.load(sys.stdin)['category'])")"

# TRY/RECOVER
echo "[TRY]"
APR=$(curl -sf -X POST "$BASE/expenses/$EXP_ID/approve" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" -d '{}')
check "approve sets true" "True" \
  "$(echo "$APR" | python3 -c "import sys,json; print(json.load(sys.stdin)['approved'])")"
check "edit approved 409" "409" \
  "$(curl -so /dev/null -w '%{http_code}' -X PUT "$BASE/expenses/$EXP_ID" \
     -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
     -d '{"title":"Modified"}')"

# RATE LIMIT
echo "[RATE LIMIT]"
RL_STATUS=200
for i in $(seq 1 12); do
  RL_STATUS=$(curl -so /dev/null -w '%{http_code}' -X POST "$BASE/expenses" \
    -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
    -d '{"title":"RL","amount":1,"category":"misc"}')
done
check "rate limit 429" "429" "$RL_STATUS"

# CACHE
echo "[CACHE]"
L1=$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")
L2=$(curl -sf "$BASE/expenses" -H "Authorization: Bearer $TOKEN_A" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")
check "cached same total" "$L1" "$L2"

# SURFACE
echo "[SURFACE]"
check "surface /api/v1/me" "user-a" \
  "$(curl -sf "$BASE/api/v1/me" -H "Authorization: Bearer $TOKEN_A" \
     | python3 -c "import sys,json; print(json.load(sys.stdin)['user_id'])")"
check "surface /api/v1/expenses" "true" \
  "$(curl -sf "$BASE/api/v1/expenses" -H "Authorization: Bearer $TOKEN_A" \
     | python3 -c "import sys,json; print('true' if 'total' in json.load(sys.stdin) else 'false')")"
check "surface /api/v1/summary" "user-a" \
  "$(curl -sf "$BASE/api/v1/summary" -H "Authorization: Bearer $TOKEN_A" \
     | python3 -c "import sys,json; print(json.load(sys.stdin)['user_id'])")"

# UPDATE / DELETE
echo "[UPDATE/DELETE]"
UPD=$(curl -sf -X PUT "$BASE/expenses/$EXP3_ID" \
  -H "Authorization: Bearer $TOKEN_A" -H "Content-Type: application/json" \
  -d '{"title":"Chair Updated","note":"New note"}')
check "update title" "Chair Updated" \
  "$(echo "$UPD" | python3 -c "import sys,json; print(json.load(sys.stdin)['title'])")"
check "delete 204" "204" \
  "$(curl -so /dev/null -w '%{http_code}' -X DELETE "$BASE/expenses/$EXP3_ID" \
     -H "Authorization: Bearer $TOKEN_A")"
check "deleted 404" "404" \
  "$(curl -so /dev/null -w '%{http_code}' "$BASE/expenses/$EXP3_ID" \
     -H "Authorization: Bearer $TOKEN_A")"

# METRICS
echo "[METRICS]"
check "metrics" "true" \
  "$(curl -sf "$BASE/metrics" | grep -q axis_request && echo true || echo false)"

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ] && exit 0 || exit 1
