#!/usr/bin/env bash
# docs/data-model.md's check (scripts/data-model-check.mjs, Arugula #751).
# It fails on a table that's gone from tables.toml, a path that's gone, an ID
# prefix nothing mints outside tests, and a "Don't say" word in the MCP tool
# list or the CLI's help, but not in an allowed phrase, and not in a doc
# comment clap never shows. Against a small fake checkout.
#
#   scripts/tests/data-model-check.sh
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

cd "$tmp"
mkdir -p scripts docs crates/control crates/a/src crates/a/tests crates/cli/src crates/daemon/src crates/arugulad-mcp/tests/fixtures
cp "$here"/data-model-check.mjs scripts/
cat >docs/data-model.md <<'EOF'
# The data model

| Noun | Truth | ID |
|---|---|---|
| **Thing** | Platform `things`, in [crates/a/src/lib.rs](../crates/a/src/lib.rs) and `crates/a/src/lib.rs:1-2` | `abc_` |

## Don't say

| Don't say | For | Say | Allowed in |
|---|---|---|---|
| box, boxes | A hosted machine | machine | `hud box`, `user@box` |

## After
EOF
printf '[[table]]\nname = "things"\n' >crates/control/tables.toml
printf 'fn id() -> String { format!("abc_{}", 1) }\n' >crates/a/src/lib.rs
printf '{"tools": [{"name": "show", "description": "A hud box as a block"}]}\n' >crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json
cat >crates/cli/src/main.rs <<'EOF'
/// Drive the daemon.
#[derive(Parser)]
struct Cli {
    /// Talk to `user@box` over ssh.
    #[arg(long)]
    ssh: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List panes.
    Ls,
}

/// Run a command on the box: not help, clap never shows it.
fn run() {}
EOF
: >crates/daemon/src/args.rs

check() { node scripts/data-model-check.mjs --check 2>&1; }
fails() { # name text-it-says
  if out=$(check); then bad "$1 should fail"; else
    case $out in *"$2"*) ok "$1 fails, saying so" ;; *) bad "$1: wrong message: $out" ;; esac
  fi
}

if out=$(check); then ok "a clean checkout passes"; else bad "a clean checkout should pass: $out"; fi

perl -pi -e 's/name = "things"/name = "widgets"/' crates/control/tables.toml
fails "a renamed table" "\`things\` isn't a table"
perl -pi -e 's/name = "widgets"/name = "things"/' crates/control/tables.toml

mv crates/a/src/lib.rs crates/a/src/gone.rs
fails "a deleted path" "names crates/a/src/lib.rs, which isn't there"
mv crates/a/src/gone.rs crates/a/src/lib.rs

mv crates/a/src/lib.rs crates/a/tests/lib.rs
: >crates/a/src/lib.rs
fails "a prefix minted only in tests" "\`abc_\` isn't minted"
mv crates/a/tests/lib.rs crates/a/src/lib.rs

perl -pi -e 's/List panes\./List panes on a box./' crates/cli/src/main.rs
fails "a banned word in the CLI's help" '"box" in crates/cli/src/main.rs:13'
perl -pi -e 's/List panes on a box\./List panes./' crates/cli/src/main.rs

perl -pi -e 's/A hud box as a block/Its boxes as blocks/' crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json
fails "a banned word in the MCP tool list" '"boxes" in crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json'

exit $fail
