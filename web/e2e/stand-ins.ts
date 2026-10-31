// The stand-in programs a fake fleet's panes run (M26): `cargo`, `npm`,
// `journalctl`, `nvim`, an agent's `claude` and the rest, from a bin
// directory first on PATH, printing like the real ones. Shared by the
// swarm's tests (fake-fleet.ts) and the demo fleet (demo-fleet.ts, #399).

import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** The stand-in programs. */
export const BIN: Record<string, string> = {
  cargo: `#!/bin/bash
# cargo stand-in: build and test print like the real thing.
case "$*" in
  *"-p relay"*) for i in 1 2 3 4 5 6 7; do echo "test grid::reflow_$i ... ok"; sleep 0.5; done
            echo "test relay::expires ... FAILED"; echo "test result: FAILED. 6 passed; 1 failed"; exit 101 ;;
  *--release*) for i in $(seq 1 11); do echo "   Compiling crate-$i v0.1.$i"; sleep 0.5; done; echo "    Finished release"; exit 0 ;;
  test*) while true; do echo "test vt::parser::case_$RANDOM ... ok"; sleep 0.7; done ;;
  *) while true; do echo "   Compiling serde-$RANDOM v1.0.0"; sleep 0.4; done ;;
esac
`,
  npm: `#!/bin/bash
echo "  VITE ready in 312 ms"; while true; do echo "GET /api/session 200 $((RANDOM % 40))ms"; sleep 1.1; done
`,
  journalctl: `#!/bin/bash
while true; do echo "arugulad: pane %$((RANDOM % 400)) output $((RANDOM % 90)) lines"; sleep 2.3; done
`,
  nvim: `#!/bin/bash
echo "-- NORMAL --"; sleep 100000
`,
  pytest: `#!/bin/bash
while true; do echo "tests/test_$((RANDOM % 40)).py::test_case_$RANDOM PASSED"; sleep 0.$((RANDOM % 9 + 1)); done
`,
  go: `#!/bin/bash
case "$1" in
  test) while true; do echo "ok  	example.com/svc/pkg$((RANDOM % 20))	0.$((RANDOM % 900))s"; sleep 1.$((RANDOM % 9)); done ;;
  *) while true; do echo "building example.com/svc/cmd$((RANDOM % 9))"; sleep 0.$((RANDOM % 6 + 2)); done ;;
esac
`,
  make: `#!/bin/bash
while true; do echo "cc -O2 -c src/mod_$((RANDOM % 80)).c"; sleep 0.$((RANDOM % 5 + 1)); done
`,
  docker: `#!/bin/bash
while true; do echo "web-1  | $(date +%T) GET /health 200"; sleep $((RANDOM % 4 + 2)); done
`,
  tail: `#!/bin/bash
while true; do echo "$(date +%T) worker[$((RANDOM % 8))]: job $RANDOM done"; sleep $((RANDOM % 6 + 1)).$((RANDOM % 9)); done
`,
  uvicorn: `#!/bin/bash
echo "INFO:     Uvicorn running on http://127.0.0.1:8000"; while true; do echo "INFO:     127.0.0.1 - \\"GET /items/$RANDOM HTTP/1.1\\" 200"; sleep $((RANDOM % 3)).$((RANDOM % 9)); done
`,
  vim: `#!/bin/bash
echo "-- INSERT --"; sleep 100000
`,
  claude: `#!/bin/bash
# claude stand-in: works a little, asks through the real hooks, then
# waits for follow-ups (what Claude Code's settings would run).
mode=\${1:-perm}; what=\${2:-cargo test}; sid="fake-$$"
echo "● Read src/main.rs"; sleep 0.3
if [ "$mode" = ask ]; then
  arugula ask < "$FAKE_FIXTURES/s13-hook-ask.json" | head -c 160; echo
elif [ "$mode" = perm ]; then
  printf '{"hook_event_name":"PreToolUse","session_id":"%s","tool_name":"Bash","tool_input":{"command":"%s"},"tool_use_id":"toolu_%s"}' "$sid" "$what" "$$" | arugula hook
  printf '{"hook_event_name":"PermissionRequest","session_id":"%s","tool_name":"Bash","tool_input":{"command":"%s"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"%s"}],"behavior":"allow","destination":"localSettings"}]}' "$sid" "$what" "$what" | arugula hook
fi
echo "● Done"
while true; do printf '{"hook_event_name":"Stop","session_id":"%s"}' "$sid" | arugula inbox 2>&1 | sed 's/^/● got: /'; done
`,
};

/** Write the stand-ins into `dir`, executable. */
export function writeStandIns(dir: string) {
  mkdirSync(dir, { recursive: true });
  for (const [name, body] of Object.entries(BIN)) {
    writeFileSync(join(dir, name), body);
    chmodSync(join(dir, name), 0o755);
  }
}
