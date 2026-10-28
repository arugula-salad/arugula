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
- [aru-103](docs/decisions/aru-103-the-deploy-event-log-is-append-only-and-anyone-c.md): The deploy event log is append-only, and anyone can verify its approvals

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
- [aru-080](docs/decisions/aru-080-the-hub-signs-people-in-to-the-platform-page.md): The hub signs people in to the platform page
- [aru-085](docs/decisions/aru-085-sending-as-a-person-uses-a-one-use-user-token-fa.md): Sending as a person uses a one-use user token, falling back to their machine
- [aru-086](docs/decisions/aru-086-private-items-are-shown-per-viewer-by-their-own.md): Private items are shown per viewer, by their own GitHub access
- [aru-095](docs/decisions/aru-095-control-enrolls-hosted-machines-under-a-project.md): Control enrolls hosted machines under a project delegation
- [aru-099](docs/decisions/aru-099-one-standing-grant-per-project-starts-runs.md): One standing grant per project starts runs
- [aru-101](docs/decisions/aru-101-a-deploy-needs-an-owner-device-s-approval-and-ag.md): A deploy needs an owner device's approval, and agents never approve
- [aru-106](docs/decisions/aru-106-the-hub-is-an-issuer-one-identity-for-every-prod.md): The hub is an issuer: one identity for every product
- [aru-113](docs/decisions/aru-113-accounts-link-by-numeric-github-id-old-sign-ins.md): Accounts link by numeric GitHub id; old sign-ins run 90 days beside the new
- [aru-121](docs/decisions/aru-121-a-person-s-agent-starts-and-answers-sessions-by-grant.md): A person's agent starts and answers sessions as them, by grant

## Clients

- [aru-026](docs/decisions/aru-026-the-daemon-serves-the-web-client-on-purpose.md): The daemon serves the web client, on purpose
- [aru-027](docs/decisions/aru-027-no-client-links-daemon-code.md): No client links daemon code
- [aru-028](docs/decisions/aru-028-the-web-client-and-the-desktop-app-are-the-clients.md): The web client and the desktop app are the clients
- [aru-029](docs/decisions/aru-029-the-tui-and-tmux-cc-are-removed.md): The TUI and tmux -CC are removed
- [aru-030](docs/decisions/aru-030-the-web-client-draws-terminals-through-one-file.md): The web client draws terminals through one file
- [aru-031](docs/decisions/aru-031-the-desktop-app-is-a-window-and-a-supervisor-the-daemon.md): The desktop app is a window and a supervisor; the daemon stays a service
- [aru-032](docs/decisions/aru-032-clients-run-ssh-daemons-don-t.md): Clients run ssh; daemons don't
- [aru-033](docs/decisions/aru-033-on-a-computer-one-workspace-a-phone-keeps-its-own.md): On a computer, one workspace; a phone keeps its own
- [aru-079](docs/decisions/aru-079-the-platform-page-is-on-app-arugula-io-not-on-th.md): The platform page is on app.arugula.io, not on the keys' origin
- [aru-081](docs/decisions/aru-081-push-stays-on-the-hub-s-app-a-push-opens-the-pla.md): Push stays on the hub's app; a push opens the platform page
- [aru-082](docs/decisions/aru-082-platform-urls-are-ids-with-cosmetic-slugs.md): Platform URLs are ids with cosmetic slugs
- [aru-083](docs/decisions/aru-083-secrets-stay-in-fragments.md): Secrets stay in fragments
- [aru-118](docs/decisions/aru-118-hints-come-from-facts-once-per-account.md): Hints come from facts, once per account

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
- [aru-087](docs/decisions/aru-087-the-daemon-s-forge-reader-stays-for-unlinked-rep.md): The daemon's forge reader stays for unlinked repos; linked repos' blocks are pins
- [aru-090](docs/decisions/aru-090-threads-hang-off-any-object.md): Threads hang off any object
- [aru-091](docs/decisions/aru-091-agents-are-in-conversations.md): Agents are in conversations
- [aru-094](docs/decisions/aru-094-hosted-machines-pause-only-between-turns.md): Hosted machines pause only between turns
- [aru-096](docs/decisions/aru-096-hosted-inference-strangers-bring-their-own-key.md): Hosted inference: strangers bring their own key
- [aru-111](docs/decisions/aru-111-a-hosted-mcp-endpoint-serves-agents-with-no-mach.md): A hosted MCP endpoint serves agents with no machine
- [aru-112](docs/decisions/aru-112-the-claude-credential-broker-and-its-metering-mo.md): The Claude credential broker and its metering move to the platform
- [aru-115](docs/decisions/aru-115-a-thread-s-agent-session-runs-on-the-project-s-h.md): A thread's agent session runs on the project's hosted box
- [aru-116](docs/decisions/aru-116-a-board-is-the-project-s-tracker-and-its-object-daemons.md): A board is the project's tracker, a platform object daemons report agent blocks to
- [aru-122](docs/decisions/aru-122-boards-manage-a-project-s-issues-wherever-they-live.md): Boards manage a project's issues, wherever they live; an epic is a board

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
- [aru-084](docs/decisions/aru-084-the-platform-mirrors-linked-github-repos-metadat.md): The platform mirrors linked GitHub repos: metadata, with content opt-in
- [aru-088](docs/decisions/aru-088-a-project-owns-numbered-issues.md): A project owns numbered issues
- [aru-089](docs/decisions/aru-089-taking-custody-of-issues-stays-reversible.md): Taking custody of issues stays reversible
- [aru-092](docs/decisions/aru-092-a-hosted-machine-is-arugulad-in-a-rented-sandbox.md): A hosted machine is arugulad in a rented sandbox
- [aru-093](docs/decisions/aru-093-the-substrate-is-built-to-e2b-s-api.md): The substrate is built to E2B's API
- [aru-097](docs/decisions/aru-097-public-previews-live-on-a-registrable-domain-of.md): Public previews live on a registrable domain of their own
- [aru-098](docs/decisions/aru-098-a-run-is-a-platform-object-on-a-hosted-workspace.md): A run is a platform object on a hosted workspace
- [aru-100](docs/decisions/aru-100-run-logs-on-hosted-workspaces-are-readable-by-th.md): Run logs on hosted workspaces are readable by the platform
- [aru-102](docs/decisions/aru-102-a-driver-is-a-run-with-a-credential.md): A driver is a run with a credential
- [aru-105](docs/decisions/aru-105-arugula-is-the-platform-s-home-experimental-s-d3.md): arugula is the platform's home; experimental's D3 is retired
- [aru-107](docs/decisions/aru-107-the-platform-is-the-one-store-for-users-project.md): The platform is the one store for users' project secrets; Infisical stays the operator's
- [aru-108](docs/decisions/aru-108-three-front-doors-the-platform-page-and-studio-a.md): Three front doors: the platform page, and studio and hud as clients
- [aru-109](docs/decisions/aru-109-studio-is-a-full-client-sign-in-with-arugula-app.md): studio is a full client: Sign in with Arugula, apps by project
- [aru-110](docs/decisions/aru-110-hud-is-an-in-app-client-platform-assertions-a-pr.md): hud is an in-app client: platform assertions, a project roster
- [aru-114](docs/decisions/aru-114-experimental-is-archived.md): experimental is archived
- [aru-118](docs/decisions/aru-118-a-project-has-docs-of-its-own-on-the-platform.md): A project has Docs of its own, on the platform
- [aru-119](docs/decisions/aru-119-an-account-can-reset-its-devices-from-a-fresh-github-sign-in.md): An account can reset its devices from a fresh GitHub sign-in, after a wait
- [aru-120](docs/decisions/aru-120-a-shared-doc-s-page-loads-its-fonts-from-its-own-origin.md): A shared Doc's page loads its fonts from its own origin

## Build and release

- [aru-063](docs/decisions/aru-063-one-switch-for-what-a-stranger-doesn-t-get-the-labs-flag.md): One switch for what a stranger doesn't get: the `labs` flag
- [aru-064](docs/decisions/aru-064-a-flag-for-each-labs-feature-and-developer-settings-to-turn.md): A flag for each Labs feature, and Developer settings to turn them on
- [aru-065](docs/decisions/aru-065-each-flag-gates-its-feature-not-just-its-listing.md): Each flag gates its feature, not just its listing
- [aru-066](docs/decisions/aru-066-a-retired-flag-is-a-name-that-isn-t-one.md): A retired flag is a name that isn't one
- [aru-067](docs/decisions/aru-067-huddles-are-removed.md): Huddles are removed
- [aru-068](docs/decisions/aru-068-control-pushes-labs-flags-the-machine-overrides.md): Control pushes Labs flags; the machine overrides
- [aru-069](docs/decisions/aru-069-zig-0-16-builds-ghostty-the-toolchain-is-pinned.md): Zig 0.16 builds Ghostty; the toolchain is pinned
- [aru-070](docs/decisions/aru-070-windows-is-a-full-platform-built-on-github-s-runners.md): Windows is a full platform, built on GitHub's runners
- [aru-104](docs/decisions/aru-104-drivers-come-in-order-chant-to-fly-then-apps-pla.md): Drivers come in order: chant to Fly, then apps-platform; planter in step 8
- [aru-117](docs/decisions/aru-117-four-labs-flags-stay-renamed-the-rest-graduate-or-retire.md): Four Labs flags stay, renamed; the rest graduate or retire
