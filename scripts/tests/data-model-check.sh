#!/usr/bin/env bash
# docs/data-model.md's check (scripts/data-model-check.mjs, Arugula #751).
# It fails on a path that's gone (but not one that moved to the cloud's
# repo), and a "Don't say" word in the MCP tool list or the CLI's help, but
# not in an allowed phrase, and not in a doc comment clap never shows, and
# on reading fewer help texts than its floor. Table names and ID prefixes
# are checked in the cloud's repo.
# Against a small fake checkout.
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
mkdir -p scripts docs crates/a/src crates/a/tests crates/cli/src crates/daemon/src crates/arugulad-mcp/tests/fixtures
cp "$here"/data-model-check.mjs "$here"/cloud-paths.mjs scripts/
# The fake CLI's help is 5 texts, not the real one's hundreds.
perl -pi -e 's/^const minHelp = \d+;/const minHelp = 5;/' scripts/data-model-check.mjs
grep -q '^const minHelp = 5;' scripts/data-model-check.mjs || bad "the test couldn't set minHelp"
cat >docs/data-model.md <<'EOF'
# The data model

| Noun | Truth | ID |
|---|---|---|
| **Thing** | Platform `things`, in [crates/a/src/lib.rs](../crates/a/src/lib.rs), `crates/a/src/lib.rs:1-2` and `crates/platform/src/things.rs` | `abc_` |

## Don't say

| Don't say | For | Say | Allowed in |
|---|---|---|---|
| box, boxes | A hosted machine | machine | `hud box`, `user@box` |

## After
EOF
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
    #[command(subcommand)]
    Cloud(CloudCmd),
}

/// Cloud workspaces.
#[derive(
    Debug,
    clap::Subcommand,
)]
enum CloudCmd {
    /// Rebuild a workspace's machine.
    Rebuild,
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

mv crates/a/src/lib.rs crates/a/src/gone.rs
fails "a deleted path" "names crates/a/src/lib.rs, which isn't there"
mv crates/a/src/gone.rs crates/a/src/lib.rs

perl -pi -e 's/a workspace.s machine/a box/' crates/cli/src/main.rs
fails "a banned word in a multi-line derive's enum" '"box" in crates/cli/src/main.rs:25'
perl -pi -e 's/a box\./a workspace'"'"'s machine./' crates/cli/src/main.rs
if out=$(check); then ok "and with it gone, a pass"; else bad "after the multi-line derive case: $out"; fi

perl -0pi -e 's/#\[derive\(\n    Debug,\n    clap::Subcommand,\n\)\]\n//' crates/cli/src/main.rs
fails "help read under the floor" "read only 3 help texts"
perl -0pi -e 's/(\/\/\/ Cloud workspaces\.\n)/$1#[derive(\n    Debug,\n    clap::Subcommand,\n)]\n/' crates/cli/src/main.rs
if out=$(check); then ok "and back to 5, a pass"; else bad "after the floor case: $out"; fi

perl -pi -e 's/List panes\./List panes on a box./' crates/cli/src/main.rs
fails "a banned word in the CLI's help" '"box" in crates/cli/src/main.rs:13'
perl -pi -e 's/List panes on a box\./List panes./' crates/cli/src/main.rs

perl -pi -e 's/A hud box as a block/Its boxes as blocks/' crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json
fails "a banned word in the MCP tool list" '"boxes" in crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json'

exit $fail
