# Security

Arugula runs your terminals, and its hosted service (control) relays
between your devices, so security reports matter a lot to us.

## Reporting a vulnerability

Please don't open a public issue. Report it privately instead:

- **GitHub:** [report a vulnerability](https://github.com/arugula-salad/arugula/security/advisories/new)
  (Security → Advisories → *Report a vulnerability*), or
- **Email:** security@arugula.io

Include what you found, how to reproduce it, and the version
(`arugula --version`) or the date, for control. You'll hear back within
three days. We'll agree a disclosure date with you once a fix is out, and
credit you in the advisory unless you'd rather not be named.

## In scope

- The daemon (`arugulad`), the CLI (`arugula`) and the web client it
  serves.
- The desktop app.
- control: control.arugula.io and its relay.
- The install script at arugula.io/install.sh and the release
  artifacts.

Please test against your own machines and accounts only. Don't access other
people's data, degrade the hosted service, or run automated scans against
control at volume.

## Supported versions

Fixes go into the latest release. Update with the same method you installed
with (see the README).

## Pre-launch review (October 2026)

Before the public launch, Arugula's code was reviewed for security at
0.65.4. The review was internal: two AI code reviewers, Claude (Anthropic)
and Codex (OpenAI), read the code independently and then checked each
other's findings, and the maintainer decided what to fix. It was a reading
of the source: nothing was fuzzed or attacked live, and **no outside audit
has been done**.

It found problems in remote access, trust and revocation, local sign-in and
link handling. The high-severity ones are fixed in 0.66.0 and 0.66.4 and
published as advisories:
[GHSA-g773-3j92-r8hm](https://github.com/arugula-salad/arugula/security/advisories/GHSA-g773-3j92-r8hm),
[GHSA-q95x-w34r-6hjj](https://github.com/arugula-salad/arugula/security/advisories/GHSA-q95x-w34r-6hjj),
[GHSA-739v-4j4c-95vm](https://github.com/arugula-salad/arugula/security/advisories/GHSA-739v-4j4c-95vm),
[GHSA-pq99-834m-v2hw](https://github.com/arugula-salad/arugula/security/advisories/GHSA-pq99-834m-v2hw),
[GHSA-cwf8-7795-6j63](https://github.com/arugula-salad/arugula/security/advisories/GHSA-cwf8-7795-6j63) and
[GHSA-ff82-9fmq-xc3f](https://github.com/arugula-salad/arugula/security/advisories/GHSA-ff82-9fmq-xc3f).
Update to 0.66.4 or later.

**What it covered:** the daemon's access checks (host, origin, local
sign-in, Tailscale identity, sockets and pipes) and its authorization of
everything a client can ask (panes, input, files, uploads, blocks, history,
shares, MCP tools and scopes); end-to-end channels, the trust chain,
revocations, team rosters, joins and device approvals; control's sign-in,
sessions, teams, relay and push; how terminal output reaches cards,
captures and links; local listeners; the desktop app's navigation and
updater; `install.sh` and the daemon's self-update.

**What it didn't:** the platform plane's own authorization (chat, projects,
Cloud workspaces' API, runs, deploys, billing); Labs features in depth;
hosted machines' run and deploy engines; Windows-specific behaviour and
`install.ps1`; the browser's Noise implementation and block bootstrap in
depth; release workflows and CI secrets; cryptographic verification and
dependencies; fuzzing or any live testing.

**What still assumes an honest control** is set out in
[docs/control-e2e.md](docs/control-e2e.md#what-holds-against-control). In
short: control serves the web client, so you trust that page (a daemon's
own page and the CLI don't come from control); a few things are trusted
the first time they're seen, such as an account's first device; control can
withhold or delay what a machine hasn't received yet, lock a team, or
refuse service, but it can't undo what a machine already holds, unlock a
team, or add a reader no owner's device signed for; hosted machines are run
by the operator; and the platform plane (chat, Cloud workspaces, Docs,
billing) is stored in plain text.

**Known and accepted:** the update to 0.66.0 doesn't rotate the local
sign-in token, so a token that could have reached another local port
through the old cookie keeps working. To make a new one, delete
`local-token` in Arugula's state directory and restart the daemon; programs
that saved the old token need the new one.
