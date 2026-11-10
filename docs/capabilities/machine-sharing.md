---
id: "machine-sharing"
title: "Machine sharing"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/daemon/src/ops/acl.rs", "crates/arugulad-invite/src/ops.rs", "crates/arugulad-invite/src/tool.rs", "crates/daemon/src/ops/shares.rs", "crates/control/src/teams.rs", "crates/cli/src/cmd/access.rs", "crates/cli/src/cmd/invite.rs", "crates/cli/src/cmd/share.rs", "web/src/shell/ui/people.tsx", "web/src/shell/trust/teams.tsx"]
paths: ["crates/mux/src/acl.rs", "crates/daemon/src/acl_api.rs", "crates/daemon/src/authz.rs", "crates/mux/src/mux/who_may.rs", "crates/core/src/access.rs", "crates/arugulad-invite/src", "crates/daemon/src/invite.rs", "crates/daemon/src/share.rs", "crates/e2e/src/team.rs", "crates/arugulad-labs/src/guest_ssh.rs", "crates/control/src/guest_jump.rs", "crates/proto/src/sharing.rs", "crates/daemon/src/ops/sharing.rs", "web/src/shell/ui/share-links.tsx", "web/src/shell/trust/prompts.tsx", "web/src/shell/e2e/team.ts", "web/src/shell/team-rosters.ts", "web/src/shell/share.ts"]
shared: ["crates/daemon/src/control.rs", "crates/daemon/src/access.rs", "crates/control/src/relay.rs", "crates/proto/src/op.rs", "crates/e2e/src/cert.rs", "web/src/shell/control.ts"]
tests: ["web/e2e/access.spec.ts", "web/e2e/teams.spec.ts", "web/e2e/share-links.spec.ts", "web/e2e/guests.spec.ts", "crates/arugulad/tests/integration/invite.rs", "crates/arugulad/tests/integration/share.rs", "crates/arugulad/tests/integration/guest_ssh.rs", "crates/control/src/routing_wire.rs"]
decisions: ["aru-021", "aru-023", "aru-024", "aru-025", "aru-056", "aru-060", "aru-072", "aru-083", "aru-126"]
---
# Machine sharing

## What a person can do

A machine's owner can give someone a role on one session, invite them to it and have them told, make a read-only link, or put the whole machine in a team. Tailnet logins work without control. Accounts, teams and links that open logged out need the machine joined to control. An ssh invite also needs Guest SSH access turned on by the owner (`sharing.json`, off by default) and a build with Labs.

## Where it runs

Session grants live on the machine. `POST /api/acl` (`AclSet`, `Access::Owner`) reaches `acl_api::set`, which writes `acl.json` through `Acl::set_full` and appends to `audit.jsonl`. `Cmd::AclChanged` runs `acl_changed` in `crates/mux/src/mux/clients.rs`, which hangs up anyone `Acl::knows` no longer knows, and `control.poke()` tells control. A new `account:` or `team:` grant must carry the root its devices chain back to.

`POST /api/invite` (`run` and `resolve` in `arugulad-invite`) grants the role if needed and pushes that one person, reporting `sent`, `pending` or `unreachable`. It names only a tailnet login, someone already granted, or a member of a roster the daemon checked itself. An agent can't send one: MCP's `invite_person` makes a card that only the owner sends.

The checks run in the daemon. `arugula_core::access::need` says what each intent needs, and the mux applies it per message (`cant` in `who_may.rs`). `authz::check` covers HTTP, where a path with no rule is owner-only, and `Acl::role` takes the best of a person's grant, team role and shared-team role.

- A viewer watches, scrolls and reads: attach, capture, tail. Private panes stay hidden.
- An editor arranges tabs and panes. Typing also has to pass `may_drive_here`: a VM they started, a role of editor or above in the machine's own team, or a per-pane time-limited trust from the owner (`trust_with`). That trust is held in the mux's memory and isn't saved.
- A non-owner's new tab or split goes through `guest_intent`: a new tab gets a VM, and a split joins its tab's VM or gets one. A new VM takes the `sandbox-providers` flag and a provider, and counts against a per-guest quota.
- `Role::Owner` on a session adds renaming and closing it. Anything marked `Access::Owner` still needs `Principal::Owner`.

Through control, the machine still decides. It tells the hub whom it lets in (`daemon_access`, by `teams::daemon_access`). The hub routes an account only if it has a say (`teams::may_reach`, `Relations::routes`), and someone outside the owner's teams waits in `daemon_offers` until they answer at `POST /api/shares/{daemon}` (`share_answers`). `Enrolled::build` in `crates/daemon/src/control.rs` then works out who each Noise key is, checking certificates against the pinned root itself, and `Control::device` answers per channel. A revoked device's channel closes.

Teams are signed rosters in the hub's `rosters` table. Each version is signed by a device of an owner in the version before (`Roster::follows`), and control can't write one. Only owners make invites (`teams::invite`): an *Ask me first* link lasts up to 30 days and a presigned one about a day, with its one-time key in the URL fragment. `accept_invite` records a request with the link's proof, and an owner's page admits it by signing the next roster (`set_roster`, `AdmitPrompt`). Changing who the owners are takes a passkey assertion (`Roster::changes_owners`).

A machine moved into a team (`api::move_daemon`) makes the team's owners `Principal::Owner` and gives editors and viewers that role on every session (`Enrolled::build`). A locked team admits only owners. On a hosted machine a team's editors come in as owner, and a deploy's box admits nobody.

There are two kinds of read-only link. `/share/<token>` (`share.rs`) shows one terminal pane to tailnet users and local processes only (`Access::check_viewer`), and `shares.json` keeps only hashes. `POST /api/links` (`acl_api::link`) adds a `link:<hash>` viewer of one session with an X25519 key whose private half is in the page's `#link=` fragment. That viewer comes through the hub's public, rate-limited `/api/relay/link/{id}`, only while `daemon_links` says the machine has a live link. Its page says what it is and waits: it reads the key, fetches `control.json` and connects only after a click in a focused window (`ui/link-note.tsx`). Both expire, and the owner can end either sooner: the Share dialog lists both under *Links and guests* with Revoke (`shares.list`, `links.list`, `share.revoke`, `link.revoke`; `arugula shares`, `arugula links`). Revoking a session's link removes its principal, so `Cmd::AclChanged` closes its viewers' channels at once, and the machine tells control what's left (`links_until`), which drops the `daemon_links` row when nothing is. A deploy's box admits no session link: `Control::device` refuses its key there, one made before the box was told it's a deploy's included, and `POST /api/links` refuses to make one.

An ssh invite (`POST /api/guests`, `guest_ssh.rs`) is the daemon's own ssh server: the username is the token, one pane, read-only by default, and `guests.json` keeps hashes. Behind NAT it hops through `guest_jump.rs`, which sees ssh ciphertext. A deploy's box admits no guest: the server refuses the login and ends a session already open when the box is told, and the box refuses to make an invite. Guest SSH access is read at each login and shell: turned off, however it was, no invite lets anyone in, and within a second every invite ends, its guests are cut off and the server stops listening (`end_all_if_off`). Listing and revoking invites work whatever the setting; only making one needs it on.

## Compatibility

- An unreadable `acl.json` logs a warning and lets in only the owner (`Acl::open`).
- A machine from before `links.list` answers 404 or 405 to it. The Share dialog says it can't list or end a session's links until it's updated, and they still end on time.
- A presigned invite is refused with a conflict until every machine checking that team understands those rosters (`PRESIGNED_INVITES`, `behind`). Team owners can't take out a machine too old for `OWNER_MOVES`.
- What's signed stays in its old "illogical …" domains so stored signatures keep verifying (`crates/e2e/src/frozen.rs`).
- Links and requests from before one-time keys were dropped rather than kept (aru-060).
- A machine that had the old `guest-ssh` flag keeps its choice once (`sharing::migrate`).
- Grants are on the machine, so nothing changes while it's off. An invite to someone control can't reach yet is `pending`, retried for a day.
- When control is down, the last certificates it sent keep working (`crates/daemon/src/control.rs`).

## Not known

- What a shared person is promised, and who sharing is for. The code doesn't say, and the docs have moved to docs.arugula.io, which I didn't read.
- Whether "from now on" should be the one default. `/api/invite` defaults to from now on, and `/api/acl` defaults to with history.
- Whether a session-level owner should be able to share. `Role::Owner`'s comment says "share it", but `/api/acl` and `/api/invite` need the daemon's owner. `arugula access grant` accepts `owner`.
- Which plan teams need. `teams.rs` tells billing when a roster changes, and aru-062 says per seat, but I found no refusal there.
- How project roles and machine sharing meet on hosted machines. `hosted::may_reach` in `crates/control/src/hosted.rs` uses the certificate's owner and the signed team roster, not the platform's project membership rows, so it doesn't contradict aru-072. Whether a project role should ever decide who reaches a hosted machine is a question for a person.
- `web/e2e/guests.spec.ts` needs wispd and skips elsewhere, and `guest_ssh.rs`'s relay test needs the test stack.
- aru-025 names `crates/daemon/src/guest_ssh.rs`, which is now `crates/arugulad-labs/src/guest_ssh.rs`. A comment on `AclSetRequest.root` says people compare fingerprints, while `docs/control-e2e.md` describes trust on first use.
