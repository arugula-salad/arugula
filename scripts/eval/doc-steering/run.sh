#!/bin/bash
# Does the steering change which tool Claude Code reaches for? (#1486)
#
#   run.sh tools A_BIN B_BIN   # one pane per condition: does Claude list Artifact?
#   run.sh eval  A_BIN B_BIN   # 5 prompts x 2 runs x 2 conditions
#
# A_BIN and B_BIN are directories holding `arugula` and `arugulad`: A from
# origin/main, B from the branch. Each condition gets its own daemon (own
# state dir, port and socket, `docs` on) under a mktemp dir, and an interactive
# `claude` in a pane of it, with only the eval's settings (--setting-sources
# project), its own MCP config (--strict-mcp-config) and a recording
# PreToolUse hook that denies Artifact, Doc and Claude Docs tools. B's
# settings add the SessionStart `arugula hook` (B's binary, by absolute path).
# Prints one line per run: condition, prompt number, run, first tool.
set -u
mode=${1:?tools|eval}; A_BIN=$(cd "${2:?A_BIN}" && pwd); B_BIN=$(cd "${3:?B_BIN}" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
claude=$(command -v claude)
T=$(mktemp -d /tmp/docsteer.XXXXXX)
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait; rm -rf "$T"; }
trap cleanup EXIT

start() { # cond bin port
  local c=$1 bin=$2 port=$3 s=$T/$1/state
  mkdir -p "$s" "$T/$c/work"
  env -i PATH="$PATH" HOME="$HOME" "$bin/arugulad" flags docs on --state-dir "$s" >/dev/null
  env -i PATH="$PATH" HOME="$HOME" USER="$USER" LOGNAME="$USER" LANG="$LANG" TMPDIR="${TMPDIR:-/tmp}" SHELL=/bin/zsh TERM=xterm-256color ARUGULA_STATE_DIR="$s" \
    "$bin/arugulad" --listen "127.0.0.1:$port" --state-dir "$s" --no-update-check --no-claude-ide \
    >"$T/$c/daemon.log" 2>&1 &
  pids+=($!)
  for _ in $(seq 50); do [ -S "$s/sock" ] && break; sleep 0.2; done
  cat >"$T/$c/mcp.json" <<J
{"mcpServers":{"arugula":{"command":"$bin/arugula","args":["--socket","$s/sock","mcp"]}}}
J
  local ss='"SessionStart":[{"hooks":[{"type":"command","command":"'"$bin"'/arugula hook"}]}],'
  [ "$c" = A ] && ss=''
  cat >"$T/$c/settings.json" <<J
{"permissions":{"allow":["mcp__arugula__*"]},
 "hooks":{$ss"PreToolUse":[{"hooks":[{"type":"command","command":"$here/record-tool.sh"}]}]}}
J
}
start A "$A_BIN" 17691
start B "$B_BIN" 17692
export EVAL_LOG=$T/log

ar() { local c=$1 bin; shift; bin=$A_BIN; [ "$c" = B ] && bin=$B_BIN
  env -u ARUGULA_PANE ARUGULA_SOCK="$T/$c/state/sock" ARUGULA_STATE_DIR="$T/$c/state" "$bin/arugula" "$@"; }

launch() { # cond runid -> pane
  local c=$1 id=$2 pane
  pane=$(ar "$c" run --cwd "$T/$c/work" "EVAL_RUN_ID=$id EVAL_LOG=$EVAL_LOG $claude --setting-sources project --settings $T/$c/settings.json --strict-mcp-config --mcp-config $T/$c/mcp.json" | grep -o '[0-9]\+' | head -1)
  for _ in $(seq 90); do
    local s; s=$(ar "$c" capture "$pane")
    case "$s" in *"Yes, I trust"*) ar "$c" keys "$pane" Down Enter; sleep 2 ;; esac
    case "$s" in *"? for shortcuts"*|*"❯"*) break ;; esac
    sleep 1
  done
  echo "$pane"
}

if [ "$mode" = tools ]; then
  for c in A B; do
    pane=$(launch "$c" tools-$c)
    ar "$c" send --wait --timeout 120 "$pane" "List your tools' names, all of them, comma-separated, and nothing else." >/dev/null
    echo "== $c"; ar "$c" capture --scrollback "$pane" >"$T/$c.cap"
    [ -n "${KEEP:-}" ] && cp "$T/$c.cap" "$KEEP-$c.cap"
    grep -o 'Artifact[A-Za-z]*\|mcp__claude_ai_Claude_Docs__[a-z]*\|mcp__arugula__doc' "$T/$c.cap" | sort -u
    ar "$c" close "$pane" >/dev/null
  done
  exit
fi

n=0
while IFS= read -r prompt; do
  n=$((n + 1))
  for r in 1 2; do
    for c in A B; do
      id=$c-$n-$r
      pane=$(launch "$c" "$id")
      ar "$c" send --wait --timeout 240 "$pane" "$prompt" >/dev/null; code=$?
      first=$(awk -v id="$id" '$1==id && ($2 ~ /^Artifact/) {print "Artifact"; exit}
        $1==id && ($2 ~ /^mcp__claude_ai_Claude_Docs__/) {print "Claude Docs"; exit}
        $1==id && ($2 ~ /^mcp__.*doc/) {print "Doc"; exit}' "$EVAL_LOG" 2>/dev/null)
      echo "$c prompt$n run$r first=${first:-none} (send exit $code; tools: $(awk -v id="$id" '$1==id{printf "%s ", $2}' "$EVAL_LOG" 2>/dev/null))"
      ar "$c" close "$pane" >/dev/null
    done
  done
done <"$here/prompts.txt"
