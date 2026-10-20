# Decisions

<!-- Written by scripts/decisions-index.mjs (`just decisions`) from
docs/decisions/. Edit the records, not this file. -->

The decisions that still constrain the code, one record each in
[docs/decisions/](docs/decisions/). Before you change one of these, read its
record; if you change it, change the record in the same PR, and run
`just decisions` to rewrite this index.

Each record is a chant decision record ([the kind](docs/decisions/decision.kind.mjs),
[its schema](docs/decisions/decision.schema.json)). Its body says what was
decided, why, where it lives in the code today ("Where:"), and where it came
from ("From:"). Its front matter holds the repository paths the body names
as `constrains`, the "From" links as `evidence`, and the section below as
`area`. `chant workspace records --current` lists them, and the workspace
block shows them. A new decision is a new record: the next id, in the area it
belongs to (`chant workspace records new --prefix aru` allocates the id).

Milestone and spike codes (M2, S5 …) are looked up in
[docs/plan-archive.md](docs/plan-archive.md), which is the old PLAN.md kept as
written. Anything there that a record says differently has changed since;
the records are what match the code.

Not here: what each milestone built (the archive), how to build and test
([docs/development.md](docs/development.md)), what the product does
([docs.arugula.io](https://docs.arugula.io/)).

## Protocol and transports

- [aru-001](docs/decisions/aru-001-one-protocol-two-transports.md): One protocol, two transports
- [aru-002](docs/decisions/aru-002-terminal-bytes-are-offsets-into-a-log.md): Terminal bytes are offsets into a log
- [aru-003](docs/decisions/aru-003-some-names-outlive-the-rename-to-arugula.md): Some names outlive the rename to Arugula
- [aru-004](docs/decisions/aru-004-the-server-sends-whole-layouts-and-deltas-for-summaries.md): The server sends whole layouts, and deltas for summaries
- [aru-005](docs/decisions/aru-005-wire-snapshots-are-formatter-vt-bytes-plus-fix-ups-zstd.md): Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request
- [aru-006](docs/decisions/aru-006-flow-control-acks-a-window-and-a-log-that-absorbs-slow.md): Flow control: acks, a window, and a log that absorbs slow clients
- [aru-007](docs/decisions/aru-007-size-a-tab-has-one-size-set-by-its-owner.md): Size: a tab has one size, set by its owner
- [aru-008](docs/decisions/aru-008-typescript-types-are-generated-from-proto.md): TypeScript types are generated from `proto`

## State and durability

- [aru-009](docs/decisions/aru-009-the-log-is-the-truth-checkpoints-are-a-cache.md): The log is the truth; checkpoints are a cache
- [aru-010](docs/decisions/aru-010-the-state-dir-holds-secrets-0700-and-0600.md): The state dir holds secrets: 0700 and 0600
- [aru-011](docs/decisions/aru-011-layout-is-a-file-written-atomically.md): Layout is a file written atomically
- [aru-012](docs/decisions/aru-012-restart-policies-decide-what-runs-after-a-restore.md): Restart policies decide what runs after a restore
- [aru-013](docs/decisions/aru-013-panes-outlive-the-daemon.md): Panes outlive the daemon

## The mux and panes

- [aru-014](docs/decisions/aru-014-one-task-decides-the-order-of-every-change.md): One task decides the order of every change
- [aru-015](docs/decisions/aru-015-the-model-is-pure-core-has-no-i-o.md): The model is pure: `core` has no I/O
- [aru-016](docs/decisions/aru-016-the-terminal-engine-sits-behind-vtengine.md): The terminal engine sits behind `VtEngine`
- [aru-017](docs/decisions/aru-017-the-mux-names-no-integration.md): The mux names no integration
- [aru-018](docs/decisions/aru-018-a-block-is-a-leaf-with-a-type-a-terminal-is-the-first-type.md): A block is a leaf with a type; a terminal is the first type
- [aru-019](docs/decisions/aru-019-layout-belongs-to-a-host-a-remote-block-references-a-pane.md): Layout belongs to a host; a remote block references a pane elsewhere
- [aru-020](docs/decisions/aru-020-shell-integration-is-injected-the-way-ghostty-does-it.md): Shell integration is injected the way Ghostty does it

## Access and identity

- [aru-021](docs/decisions/aru-021-one-function-says-what-an-intent-needs.md): One function says what an intent needs
- [aru-022](docs/decisions/aru-022-operations-are-declared-once-and-listed-in-two-checked-in.md): Operations are declared once, and listed in two checked-in files
- [aru-023](docs/decisions/aru-023-who-can-reach-a-daemon.md): Who can reach a daemon
- [aru-024](docs/decisions/aru-024-a-guest-types-on-your-machine-only-by-trust-or-in-a-vm.md): A guest types on your machine only by trust, or in a VM
- [aru-025](docs/decisions/aru-025-only-the-owner-s-agent-reaches-panes-over-ssh.md): Only the owner's agent reaches panes over ssh
- [aru-073](docs/decisions/aru-073-grants-not-impersonation.md): Grants, not impersonation
- [aru-078](docs/decisions/aru-078-the-hub-stays-the-identity-provider-passkeys-are.md): The hub stays the identity provider; passkeys are bound to control.arugula.io

## Clients

- [aru-026](docs/decisions/aru-026-the-daemon-serves-the-web-client-on-purpose.md): The daemon serves the web client, on purpose
- [aru-027](docs/decisions/aru-027-no-client-links-daemon-code.md): No client links daemon code
- [aru-028](docs/decisions/aru-028-the-web-client-and-the-desktop-app-are-the-clients.md): The web client and the desktop app are the clients
- [aru-029](docs/decisions/aru-029-the-tui-and-tmux-cc-are-removed.md): The TUI and tmux -CC are removed
- [aru-030](docs/decisions/aru-030-the-web-client-draws-terminals-through-one-file.md): The web client draws terminals through one file
- [aru-031](docs/decisions/aru-031-the-desktop-app-is-a-window-and-a-supervisor-the-daemon.md): The desktop app is a window and a supervisor; the daemon stays a service
- [aru-032](docs/decisions/aru-032-clients-run-ssh-daemons-don-t.md): Clients run ssh; daemons don't
- [aru-033](docs/decisions/aru-033-on-a-computer-one-workspace-a-phone-keeps-its-own.md): On a computer, one workspace; a phone keeps its own

## Agents and blocks

- [aru-034](docs/decisions/aru-034-an-agent-block-is-an-acp-client.md): An agent block is an ACP client
- [aru-035](docs/decisions/aru-035-a-chat-block-is-an-agent-block-with-every-tool-off.md): A chat block is an agent block with every tool off
- [aru-036](docs/decisions/aru-036-a-claude-code-block-logs-in-as-whoever-started-it.md): A Claude Code block logs in as whoever started it
- [aru-037](docs/decisions/aru-037-attention-comes-from-signals-hooks-and-a-fallback.md): Attention comes from signals, hooks and a fallback
- [aru-038](docs/decisions/aru-038-mcp-is-served-by-the-daemon-with-the-api-s-own-checks.md): MCP is served by the daemon, with the API's own checks
- [aru-039](docs/decisions/aru-039-the-claude-code-plugin-calls-the-installed-cli-its-hooks.md): The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones
- [aru-040](docs/decisions/aru-040-each-browser-block-gets-its-own-origin.md): Each browser block gets its own origin
- [aru-041](docs/decisions/aru-041-forge-blocks-use-the-person-s-own-cli-and-agents-draft.md): Forge blocks use the person's own CLI, and agents draft while people send
- [aru-042](docs/decisions/aru-042-uploads-land-in-a-per-pane-folder-on-the-pane-s-host.md): Uploads land in a per-pane folder on the pane's host
- [aru-043](docs/decisions/aru-043-a-workspace-block-asks-chant-and-keeps-a-few-reads-of-its.md): A workspace block asks chant, and keeps a few reads of its own
- [aru-044](docs/decisions/aru-044-a-workspace-block-s-graph-is-behold-run-by-the-block.md): A workspace block's graph is behold, run by the block
- [aru-045](docs/decisions/aru-045-a-recipe-is-a-claude-code-subagent-file-worn-on-this-host.md): A recipe is a Claude Code subagent file, worn on this host
- [aru-046](docs/decisions/aru-046-cloud-execution-is-owned-by-arugula-control.md): Cloud execution is owned by Arugula control
- [aru-047](docs/decisions/aru-047-agents-on-a-machine-drive-its-owner-s-cloud-workspaces.md): Agents on a machine drive its owner's Cloud workspaces
- [aru-048](docs/decisions/aru-048-daemon-side-managoat-is-removed.md): Daemon-side Managoat is removed
- [aru-049](docs/decisions/aru-049-a-machine-reaches-another-s-agents-as-its-account-through.md): A machine reaches another's agents as its account, through the relay
- [aru-050](docs/decisions/aru-050-a-task-waits-for-the-machine-s-own-account-on-a-card-held.md): A task waits for the machine's own account, on a card held by a block
- [aru-051](docs/decisions/aru-051-chat-lives-in-control-in-plain-text-terminal-content-doesn-t.md): Chat lives in control, in plain text; terminal content doesn't
- [aru-052](docs/decisions/aru-052-agents-are-out-of-chat.md): Agents are out of chat
- [aru-053](docs/decisions/aru-053-chat-calls-the-owner-by-a-short-name-ledgers-keep-their.md): Chat calls the owner by a short name; ledgers keep their login

## Federation and control

- [aru-054](docs/decisions/aru-054-every-daemon-is-a-peer-control-is-the-only-front-door.md): Every daemon is a peer; control is the only front door
- [aru-055](docs/decisions/aru-055-control-can-refuse-service-but-can-t-read.md): Control can refuse service but can't read
- [aru-056](docs/decisions/aru-056-trust-is-a-chain-of-device-approvals.md): Trust is a chain of device approvals
- [aru-057](docs/decisions/aru-057-a-refused-approval-names-the-check-and-isn-t-a-turn-down.md): A refused approval names the check, and isn't a turn-down
- [aru-058](docs/decisions/aru-058-a-machine-s-join-link-carries-a-secret-the-account-isn-t.md): A machine's join link carries a secret; the account isn't compared
- [aru-059](docs/decisions/aru-059-a-new-browser-shows-a-code-without-its-answer-it-asks-to.md): A new browser shows a code; without its answer it asks to check the account
- [aru-060](docs/decisions/aru-060-every-team-invite-link-carries-a-one-time-key-admitting.md): Every team invite link carries a one-time key; admitting someone compares nothing
- [aru-061](docs/decisions/aru-061-a-passkey-with-prf-is-a-device-one-wherever-it-syncs.md): A passkey with PRF is a device: one wherever it syncs
- [aru-062](docs/decisions/aru-062-control-s-code-is-open-only-the-hosted-control-is-supported.md): Control's code is open, only the hosted control is supported; the tailnet stays first class
- [aru-071](docs/decisions/aru-071-two-planes-two-trust-rules.md): Two planes, two trust rules
- [aru-072](docs/decisions/aru-072-the-project-is-the-unit-of-platform-sharing.md): The project is the unit of platform sharing
- [aru-074](docs/decisions/aru-074-one-vocabulary-project-with-prj-ids.md): One vocabulary: Project, with prj_ ids
- [aru-075](docs/decisions/aru-075-control-is-two-processes-a-hub-and-a-platform.md): Control is two processes: a hub and a platform
- [aru-076](docs/decisions/aru-076-platform-paths-stay-on-control-arugula-io-forever.md): Platform paths stay on control.arugula.io forever, behind the hub's gateway
- [aru-077](docs/decisions/aru-077-the-platform-s-database-is-fly-managed-postgres.md): The platform's database is Fly Managed Postgres, moved in announced windows

## Build and release

- [aru-063](docs/decisions/aru-063-one-switch-for-what-a-stranger-doesn-t-get-the-labs-flag.md): One switch for what a stranger doesn't get: the `labs` flag
- [aru-064](docs/decisions/aru-064-a-flag-for-each-labs-feature-and-developer-settings-to-turn.md): A flag for each Labs feature, and Developer settings to turn them on
- [aru-065](docs/decisions/aru-065-each-flag-gates-its-feature-not-just-its-listing.md): Each flag gates its feature, not just its listing
- [aru-066](docs/decisions/aru-066-a-retired-flag-is-a-name-that-isn-t-one.md): A retired flag is a name that isn't one
- [aru-067](docs/decisions/aru-067-huddles-are-removed.md): Huddles are removed
- [aru-068](docs/decisions/aru-068-control-pushes-labs-flags-the-machine-overrides.md): Control pushes Labs flags; the machine overrides
- [aru-069](docs/decisions/aru-069-zig-0-16-builds-ghostty-the-toolchain-is-pinned.md): Zig 0.16 builds Ghostty; the toolchain is pinned
- [aru-070](docs/decisions/aru-070-windows-is-a-full-platform-built-on-github-s-runners.md): Windows is a full platform, built on GitHub's runners
