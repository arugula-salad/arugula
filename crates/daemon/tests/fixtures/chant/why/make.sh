#!/bin/sh
# #619's fixture: a chant workspace whose app/server.mjs has a line a
# member's agent committed in a recorded run, a line nobody has committed
# yet, a decision that constrains the file by path, and a work item on the
# member whose lease the member's agent session holds.
#
#   [RUN=<run id>] make.sh DIR CHANT [OUT]
#
# makes the workspace in DIR (empty or not there) with CHANT (an
# installed @intentius/chant's node_modules/.bin/chant; the decision and
# work kinds come from its conformance fixtures). With OUT, it writes what
# the Changes block reads there: `graph --intent app/server.mjs --json` as
# intent.json and `status local --json` as status.json, with DIR written as
# /ws. The agent's work is on the branch `work`, off `main`. RUN is its
# run's id (an agent block's own, `arugula-<block>-<ms>`, to link the run
# to that block). Recorded with chant 0.108.1.
set -eu
dir=$1 chant=$2 out=${3:-}
conf=$(cd "$(dirname "$chant")/../@intentius/chant/src/workspace/conformance" && pwd)
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
cd "$dir"
mkdir app decisions work
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com
export GIT_AUTHOR_DATE=2026-10-01T09:00:00Z GIT_COMMITTER_DATE=2026-10-01T09:00:00Z
git init -q -b main
cp "$conf/__fixture__/decisions/decision.kind.mjs" "$conf/__fixture__/decisions/decision.schema.json" decisions/
cp "$conf/__writer_fixture__/work/work.kind.mjs" "$conf/__writer_fixture__/work/work.schema.json" work/
cat > chant.workspace.json <<'END'
{
  "name": "why",
  "schema": 1,
  "members": [{ "name": "app", "dir": "app", "kind": "other", "because": "a toy server" }],
  "records": [{ "kind": "decisions/decision.kind.mjs" }, { "kind": "work/work.kind.mjs" }],
  "agents": [{ "name": "app", "member": "app" }]
}
END
cat > decisions/why-001-the-server-answers-on-one-port.md <<'END'
---
schema: 1
id: "why-001"
title: "The server answers on one port"
state: "decided"
area: "app"
source:
  issue: null
  row: null
  revision: null
question: "Which port does the server listen on?"
options:
  - id: "a"
    label: "one port, from PORT"
    how: "port() reads PORT, default 8080."
    tradeoff: "Simple."
  - id: "b"
    label: "two ports"
    how: "An admin port beside the public one."
    tradeoff: "More to secure."
choice:
  option: "a"
  reason: "One port is all the toy needs."
rejected:
  - option: "b"
    why: "Nothing needs an admin port."
supersedes: []
evidence: []
decided_by: "t"
decided_on: "2026-10-01"
reviews: []
constrains:
  - "path:app/server.mjs"
---

# The server answers on one port
END
cat > work/W-001-greet-with-punctuation.md <<'END'
---
schema: 1
id: "W-001"
title: "Greet with punctuation"
state: "open"
implements: []
needs: []
constrains:
  - "member:app"
evidence: []
acceptance:
  - id: "AC-1"
    text: "greet says hello, name!"
    verification: "unit"
owner: "t"
opened_on: "2026-10-01"
source:
  kind: "workspace"
  member: "app"
supersedes: []
---

# Greet with punctuation
END
cat > app/server.mjs <<'END'
// The toy server.
export function port() {
  return 3000;
}

export function greet(name) {
  return `hello ${name}`;
}
END
git add -A
git commit -qm "the workspace"
git checkout -qb work

# A turn of the member's agent, as an agent block records it (#590): the
# run starts, its commit carries the trailers, and its end names the
# commit and the line it wrote.
run=${RUN:-arugula-7-1790848800000}
printf '{"id":"%s","harness":"claude-code","model":"claude-opus-5-5","agent":"app","startedAt":"2026-10-01T10:00:00.000Z","instruction":{"sha256":"%s"}}' \
  "$run" "$(printf 'read PORT' | shasum -a 256 | cut -d' ' -f1)" | "$chant" workspace runs start --from - >/dev/null
sed 's/  return 3000;/  return Number(process.env.PORT ?? 8080);/' app/server.mjs > app/server.new && mv app/server.new app/server.mjs
git add -A
export GIT_AUTHOR_DATE=2026-10-01T10:04:00Z GIT_COMMITTER_DATE=2026-10-01T10:04:00Z
git commit -qm "port: read PORT, default 8080" -m "Chant-Agent: app
Chant-Run: $run"
sha=$(git rev-parse HEAD)
printf '{"outcome":"end_turn","endedAt":"2026-10-01T10:05:00.000Z","usage":{"turns":1},"commits":[{"sha":"%s","hunks":[{"path":"app/server.mjs","start":3,"end":3}]}]}' \
  "$sha" | "$chant" workspace runs end "$run" --from - >/dev/null

# The agent's next piece of work, under W-001's lease, not committed yet.
"$chant" workspace work claim W-001 --holder app --ttl 3650d --json >/dev/null
sed 's/hello \${name}/hello, ${name}!/' app/server.mjs > app/server.new && mv app/server.new app/server.mjs

if [ -n "$out" ]; then
  "$chant" workspace graph --intent app/server.mjs --json | sed "s#$dir#/ws#g" > "$out/intent.json"
  "$chant" workspace status local --json | sed "s#$dir#/ws#g" > "$out/status.json"
fi
