#!/usr/bin/env bash
# The contract's compatibility rule (scripts/contract-compat.mjs, Arugula
# #807), on a small fake contract in a git repo of its own: additions pass,
# each kind of break fails naming what broke, and a Contract-break line in a
# commit message allows the break it names.
#
#   scripts/tests/contract-compat.sh
# shellcheck disable=SC2016 # the edits are JavaScript: JSON Schema's $defs and $ref
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

repo=$tmp/repo
g() { git -C "$repo" -c user.name=test -c user.email=test@example.com -c commit.gpgsign=false "$@"; }
mkdir -p "$repo/contract/schema" "$repo/contract/api" "$repo/contract/openapi"
cat >"$repo/contract/schema/chat.json" <<'EOF'
{"$defs": {
  "Room": {"type": "object", "properties": {
    "id": {"type": "string"},
    "name": {"type": "string"},
    "last": {"type": "integer", "format": "int64"},
    "kind": {"$ref": "#/$defs/Kind"},
    "tags": {"type": "array", "items": {"type": "string"}, "default": []}
  }, "required": ["id", "name"]},
  "Kind": {"oneOf": [{"enum": ["channel", "dm"], "type": "string"}, {"const": "pane", "type": "string", "description": "A pane's."}]},
  "Live": {"oneOf": [
    {"type": "object", "properties": {"type": {"const": "hello", "type": "string"}, "stream": {"type": "string"}}, "required": ["type", "stream"]},
    {"type": "object", "properties": {"type": {"const": "message", "type": "string"}, "room": {"type": "string"}}, "required": ["type", "room"]}
  ]},
  "Old": {"type": "object", "properties": {"x": {"type": "string"}}}
}}
EOF
cat >"$repo/contract/api/routes.json" <<'EOF'
{"routes": [
  {"method": "GET", "path": "/api/chat/rooms", "auth": "session", "clients": ["page", "native"]},
  {"method": "POST", "path": "/api/chat/rooms", "auth": "session", "clients": ["page", "cli", "native"]}
]}
EOF
cat >"$repo/contract/openapi/native.json" <<'EOF'
{"paths": {"/api/chat/rooms": {
  "get": {"responses": {"2XX": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Room"}}}}}},
  "post": {"requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Room"}}}}, "responses": {"2XX": {"description": "OK"}}}
}}}
EOF
g init -q -b main
g add -A
g commit -qm base
g checkout -q -b work

# edit FILE JS: run JS on the file's JSON as `d`, and write it back.
edit() { node -e 'const fs=require("fs");const f=process.argv[1];const d=JSON.parse(fs.readFileSync(f));(new Function("d",process.argv[2]))(d);fs.writeFileSync(f,JSON.stringify(d))' "$repo/contract/$1" "$2"; }
# Back to main's contract, and the branch's commits dropped.
reset() { g reset -q --hard main; }

# expect NAME pass|fail ['TEXT']: the check's verdict, and TEXT in what it says.
expect() {
  local name=$1 want=$2 text=${3:-} out st=0
  out=$(node "$here/contract-compat.mjs" --root "$repo" --base main 2>&1) || st=$?
  if [ "$want" = pass ] && [ "$st" != 0 ]; then bad "$name: failed: $out"; reset; return; fi
  if [ "$want" = fail ] && [ "$st" != 1 ]; then bad "$name: exit $st, not 1: $out"; reset; return; fi
  if [ -n "$text" ] && ! grep -qF -- "$text" <<<"$out"; then bad "$name: doesn't say '$text': $out"; reset; return; fi
  ok "$name"
  reset
}

expect "no change passes" pass "only additions"

edit schema/chat.json 'const r=d.$defs.Room.properties; r.topic={type:"string"}; d.$defs.Kind.oneOf[0].enum.push("object"); d.$defs.New={type:"object"}; d.$defs.Live.oneOf.push({type:"object",properties:{type:{const:"resync"}},required:["type"]})'
edit api/routes.json 'd.routes.push({method:"DELETE",path:"/api/chat/rooms",auth:"session",clients:["page"]}); d.routes[0].clients.push("cli")'
edit openapi/native.json 'd.paths["/api/chat/rooms"].delete={responses:{}}; d.paths["/api/chat/rooms"].post.responses["2XX"].content={"application/json":{schema:{$ref:"#/components/schemas/Room"}}}'
expect "additions pass: a field, a value, a variant, a type, a route, a client, an operation, an untyped answer typed" pass

edit schema/chat.json 'const k=d.$defs.Kind.oneOf; d.$defs.Kind.oneOf=[{enum:["channel"],type:"string"},{const:"dm",type:"string",description:"Two people."},k[1]]'
expect "an enum value documented (enum to const) passes" pass

edit api/routes.json 'd.routes.pop()'
expect "a removed route fails" fail "POST /api/chat/rooms: removed route"

edit api/routes.json 'd.routes[1].clients=["page","native"]'
expect "a route that stops serving a client fails" fail "POST /api/chat/rooms: route no longer serves cli"

edit openapi/native.json 'delete d.paths["/api/chat/rooms"].get'
expect "a removed operation fails" fail "GET /api/chat/rooms: removed operation"

edit openapi/native.json 'd.paths["/api/chat/rooms"].get.responses["2XX"].content["application/json"].schema.$ref="#/components/schemas/Old"'
expect "an operation answering another type fails" fail "GET /api/chat/rooms response: changed type: was application/json Room, now application/json Old"

edit schema/chat.json 'delete d.$defs.Room.properties.last'
expect "a removed field fails" fail "chat.Room.last: removed field"

edit schema/chat.json 'd.$defs.Room.required.push("last")'
expect "a field made required fails" fail "chat.Room.last: became required"

edit schema/chat.json 'd.$defs.Room.properties.topic={type:"string"}; d.$defs.Room.required.push("topic")'
expect "a new required field fails" fail "chat.Room.topic: added as required"

edit schema/chat.json 'd.$defs.Room.properties.name.type="integer"'
expect "a changed type fails" fail "chat.Room.name: changed type: was string, now integer"

edit schema/chat.json 'd.$defs.Room.properties.name.type=["string","null"]'
expect "a field made nullable fails" fail "chat.Room.name: changed type: was string, now null|string"

edit schema/chat.json 'd.$defs.Room.properties.last.format="int32"'
expect "a changed format fails" fail "chat.Room.last: changed type: was integer (int64), now integer (int32)"

edit schema/chat.json 'd.$defs.Room.properties.kind={$ref:"#/$defs/Old"}'
expect "a field of another type fails" fail "chat.Room.kind: changed type: was Kind, now Old"

edit schema/chat.json 'd.$defs.Room.properties.tags.items={type:"integer"}'
expect "a list of another type fails" fail "chat.Room.tags[]: changed type"

edit schema/chat.json 'd.$defs.Kind.oneOf.pop()'
expect "a removed enum value fails" fail 'chat.Kind: removed value "pane"'

edit schema/chat.json 'd.$defs.Live.oneOf.shift()'
expect "a removed variant fails" fail "chat.Live<type=hello>: removed variant"

edit schema/chat.json 'delete d.$defs.Live.oneOf[1].properties.room'
expect "a removed field of a variant fails" fail "chat.Live<type=message>.room: removed field"

edit schema/chat.json 'delete d.$defs.Old'
expect "a removed type fails" fail "chat.Old: removed type"

printf '{"$defs": {"Old": {"type": "object", "properties": {"x": {"type": "string"}}}}}\n' >"$repo/contract/schema/team.json"
g add -A && g commit -qm "Old is team's"
edit schema/chat.json 'delete d.$defs.Old'
expect "a type another module still carries passes" pass

edit schema/chat.json 'delete d.$defs.Room.properties.last'
g commit -qam "Rooms lose last

Contract-break: chat.Room.last, last read by 0.1.0, out of the window"
expect "a Contract-break line allows what it names" pass "chat.Room.last: removed field, allowed by Contract-break: chat.Room.last"

edit schema/chat.json 'delete d.$defs.Room.properties.last; delete d.$defs.Room.properties.tags'
g commit -qam "Rooms lose fields

Contract-break: chat.Room, nothing in the window reads them"
expect "a Contract-break line covers what's under it" pass "chat.Room.tags: removed field, allowed"

edit schema/chat.json 'delete d.$defs.Room.properties.last; delete d.$defs.Room.properties.tags'
g commit -qam "Rooms lose fields

Contract-break: chat.Room.last, out of the window"
expect "and only what's under it" fail "chat.Room.tags: removed field"

edit schema/chat.json 'delete d.$defs.Room.properties.last'
g commit -qam "x

Contract-break: chat.Ro, out of the window"
expect "a Contract-break line doesn't cover a name it only starts" fail "chat.Room.last: removed field"

edit schema/chat.json 'delete d.$defs.Room.properties.last'
g commit -qam "x

Contract-break: chat.Room.last"
expect "a Contract-break line without a why fails" fail "say what and why"

exit $fail
