# Arugula control

Using control (signing in, devices, joining machines, teams) is on
**[docs.arugula.io/control](https://docs.arugula.io/control/)**. Operating the
hosted one, running your own (unsupported for now, #662) and testing it are in
[control-ops.md](control-ops.md); the design in [control-e2e.md](control-e2e.md).

**A joined machine acts as its owner in their Cloud workspaces (#1094).**
Cloud workspaces are hosted workspaces (below). Control takes a daemon's
signature on their routes (list, create, open, rebuild, delete, the spec,
secrets, previews and transcripts) and scopes it to the daemon's account,
as it would that account's own session. The agents on that machine drive
them through MCP (`list kind cloud_workspaces`, `cloud`, `read_cloud`:
"Agents on hosted workspaces", below).
**A teammate with run rights on a machine can therefore drive its owner's
Cloud workspaces through the agents there.** That was decided, not
overlooked (`cloud::CloudDriver` in `crates/platform/src/cloud/mod.rs`).
The Managoat-backed Cloud workspaces that came first were removed in
M189 and M190 ([managoat.md](managoat.md)).

**That power is a grant (M130, #1126).** Every machine joined to an account
holds a `cloud.drive` grant over it: machines joined before M130 got one
when control started, and each machine that joins gets one as soon as it's
approved (from control's change feed, or the first time it drives, whichever
comes first, so a machine that just joined is never refused). The routes
above check it, so a machine without a live grant is
refused (403) with a message that names it and says how to give it back.
The owner manages them from a device logged in to the account:

```
arugula grants                  # every machine's grants, live and revoked
arugula grants revoke geek      # geek's agents can't drive your Cloud workspaces
arugula grants restore geek     # they can again (a new grant; the old one stays revoked)
```

or through control's API, as a signed-in person or a CLI device (a
machine's signature isn't accepted here): `GET /api/grants`,
`POST /api/grants/{id}/revoke`, and `POST /api/grants
{"daemon": "<machine id>", "power": "cloud.drive"}`. Grants are per machine,
since the daemon signs every MCP call as itself; narrowing one to the
owner's own agents is a later daemon change. A machine that's removed loses
its grants, and one that joins again gets a new one. Grants live in
the platform's `grants` table (`crates/platform/src/db/projects.rs`, "Grants,
not impersonation" in [DECISIONS.md](../DECISIONS.md)), and the routes are
the platform's behind the hub's gate (`crates/platform/src/grants.rs`).

**Linked repos (M163, #1209).** A GitHub repo is linked to one project, by
a member of it (the account, for a personal project; a team's owner or
editor) who is an admin or maintainer of the repo on GitHub. Control checks
that by the person's numeric GitHub id (from signing in with GitHub),
through a read-only installation token of its GitHub App. Where the App
isn't installed for the repo, the answer is a 409 with `reason:
"install_app"` and GitHub's page to install it, whose `state` is
`<project>:<owner>/<name>`; the App's setup URL is the platform page's
`/installed`, which brings the person back to the project's settings with
the repo filled in. The same machines that drive Cloud workspaces link for
their owner (a live `cloud.drive` grant), so agents can through MCP
(`list` kind `linked_repos`, `link_repo`).

```
arugula project ls                        # your personal project's linked repos
arugula project link acme/app             # --project <id or name> for another
arugula project unlink acme/app
```

Unlinking stops reading GitHub for the repo at once. So does the App's
installation being deleted or suspended, or the repo being taken out of it
(its `installation` and `installation_repositories` webhooks). The repo's
rows (items, checks, events, content, drafts and inbox rows) go within a day
(`purge_after`, `crates/platform/src/forge/links.rs`) unless it's linked
again to the same project first. Linking it to another project instead
purges the old project's rows at once.

**Hosted workspaces (M179, #1240).** A hosted workspace is `arugulad` in
a box at the platform's compute provider (E2B, `crates/platform/src/compute`),
in a project. It enrolls into the project under the project's delegation
(M178) and shows in its machines. A project's members list and open its
workspaces; its owners and editors make, rebuild and delete them, and
making a box also needs the account on `ARUGULA_HOSTED_ACCOUNTS`. The
platform's worker makes each box from the current release's template
(`ARUGULA_HOSTED_TEMPLATE` until M177's lookup), records its one-time
enrollment with the hub by sandbox id (or puts a ticket in its env, with
`ARUGULA_HOSTED_ENROLL=ticket`, for boxes that boot fresh), and deletes a box
that doesn't enroll in time. A create whose answer was lost is found by the
box's metadata, never sent twice (`crates/platform/src/hosted/`).

A box pauses only between turns (M181, aru-094). It pauses itself, memory
and disk kept, 10 minutes (`ARUGULA_HOSTED_IDLE_TIMEOUT`, in seconds) after
it was made, woken or last held. While its daemon says it's busy (an agent
turn or a foreground command), the platform's leader holds it: every few
minutes it sets the box's timeout to the idle timeout from now. Idle or
silent, nothing holds it. The platform never pauses a box whose daemon says
busy. E2B lets a box run 24 hours without a pause on Pro
(`ARUGULA_HOSTED_RUNTIME_CAP`; Hobby's is 3600 seconds). In the last
twelfth before that, the worker pauses and resumes the box at its next idle
moment, which restarts E2B's clock. A single turn that runs to the cap
pauses there: the worker wakes it at once and says so on the workspace and
in the platform's log.
`scripts/hosted-pause-gate` checks this by hand on a real control (M192's
gate): a half-hour turn nobody watches, then the pause; and a box left
overnight, paused in the morning with its panes still there.

A woken box is `waking` until its daemon dials in again, and `ready` only
then (M258, Arugula #124): opening it, the hold's wake as a turn starts, and the
pause and resume before the cap all wait on it. One whose daemon doesn't
dial in within 60 seconds (`ARUGULA_HOSTED_DIAL_WAIT`) is `failed`, saying
it needs a rebuild: its box is paused again, and the project's owners and
editors are pushed, not only whoever opened it. The platform never reaches
into a box to start the daemon (it holds no way to). The template keeps it
running instead: `arugulad.service` (`Restart=always`,
`OOMScoreAdjust=-1000`), or the start command's own loop without systemd
(`packaging/hosted/start`). Its panes and agents don't keep the OOM
protection, so the kernel kills a runaway build first.

On its first boot the box sets itself up from the spec (M183, #1244), in a
pane in a session called `setup` that anyone on the project can watch like
any pane: it clones the repos into `$HOME`, installs the packages with
apt, then runs the setup script with `bash -e` in the first repo. Its
daemon reads the spec from the hub's `GET /api/hosted/spec` (signed as the
hosted machine; the hub asks the platform) and says how it goes at `POST
/api/hosted/setup`: setting up (with the step), ready, or failed (with the
step and a fixed reason code). Control never gets the pane's text: `cloud
ls` and `open` say "failed at <step>: open the setup pane", and `--wait`
reads a failed setup's last lines from the box over the relay, end to end. A GitHub repo linked to the project is
cloned with a read-only installation token for that repo alone, minted for
the clone; any other with the workspace's `GIT_TOKEN` secret (and
`GIT_USERNAME`, else `x-access-token`). The daemon hands them to git in its
environment for the clone alone, from a file in memory the setup removes as
it reads it: nothing lands in a repo's config or on the disk. The spec's
`env` names take their values from the workspace's secrets, for the setup
script. Changing the spec shows "rebuild needed" until the box is rebuilt.
Before a rebuild the CLI asks the box, through the relay, for repos with
changes or commits on no remote branch (`GET /api/hosted/setup` on the
daemon), and stops unless `--download FILE` saves them first (a tarball of
those repos' files that aren't ignored and their `.git`, `GET
/api/hosted/setup/archive`) or `--force`.

```
arugula cloud create dev --repo https://github.com/acme/app --setup setup.sh
arugula cloud ls                          # every project's; --project <id or name>
arugula cloud open dev --wait             # wakes a paused box, says its machine and its setup
arugula cloud edit dev --setup setup.sh   # rebuild needed
echo "$TOKEN" | arugula cloud secret set dev GIT_TOKEN
arugula cloud rebuild dev --download dev.tgz   # a new box from the spec
arugula cloud delete dev
```

**Runs and their triggers (M214, platform step 7).** A run executes on one
of a project's hosted workspaces. It starts by hand, on a schedule, or on a
linked repo's event, and the platform makes it `pending` and queues its job
for the workspace's machine (M212). A **run spec** is a named run kept in
the project: the workspace, and a command or an instruction to an agent.
Its **schedules** are cron expressions (five fields, at most every 5
minutes) in a time zone. The platform fires them, so they keep ticking with
every person's machine asleep. A tick missed while the platform is down
fires once, and two platform machines or a restart never fire one twice.
Its **rules** start it on a PR opened, a check failed or a push, on a
linked repo and branch (`*` matches any run of characters). Each event
starts it once, however often GitHub delivers it. By default a rule
refuses events from outside the repo: a PR from a fork or by someone who
isn't the repo's owner, member or collaborator, and a check run whose head
isn't in the repo. A fork's branch names never match. Only an owner lets a
rule take them (`outsiders`). Each rule starts at most 20 runs an hour,
unless it says otherwise (`max_per_hour`). A schedule or a rule fires only
while the person who made it is still an owner or editor of the project,
on `ARUGULA_HOSTED_ACCOUNTS`; otherwise it passes with no run and says why.
What an event's source says is untrusted text, written by whoever caused
it: it's shown quoted, and never reaches a run's environment, command or
an agent's prompt unquoted. A push rule needs the
App subscribed to **Push** events (with *Contents: read*), which its
installers approve: until then push rules don't fire. Owners and editors on
`ARUGULA_HOSTED_ACCOUNTS` change them and start runs. Only a person or a
CLI device does: a machine's agent starts runs through MCP (M216, below),
under the project's standing grant (M211). The code is in
`crates/platform/src/triggers/` and `db/triggers.rs`.

**Run logs and artifacts (M213).** A run on a hosted workspace sends its
log (its pane's output, every byte) and the files it leaves in
`$ARUGULA_ARTIFACTS` to the run-log bucket, in chunks of about 4 MiB as it
runs and the rest when it ends, before it reports its end. The platform
reads them: a project's members read the log a page at a time (`GET
/api/runs/{run}/log?offset=N`, M216's, which `arugula runs logs` and MCP's
`read_run` use), list its artifacts with signed links (`GET
/api/projects/{project}/runs/{run}/artifacts`) and fetch one
(`…/artifacts/{name}`), with every machine asleep. Only hosted runs send
any: a person's machine never does. The bucket keeps them 30 days, and the
platform forgets its rows then too. The bucket is S3-compatible (Cloudflare
R2 for the hosted control), set with `ARUGULA_RUN_LOGS_BUCKET`,
`ARUGULA_RUN_LOGS_ENDPOINT`, `ARUGULA_RUN_LOGS_ACCESS_KEY_ID` and
`ARUGULA_RUN_LOGS_SECRET_ACCESS_KEY` (and `ARUGULA_RUN_LOGS_DAYS`, 30);
without it, a run's log stays in its pane. `arugula-platform run-logs --db
…` shows the operator the bytes kept per project. The code is in
`crates/platform/src/run_logs/` and `crates/daemon/src/run_logs.rs`.

```
arugula runs spec add nightly --workspace dev --dir /work/app -- just e2e
arugula runs schedule add nightly --cron '0 3 * * *' --tz America/Chicago
arugula runs rule add nightly --on check-failed --branch main
arugula runs start --spec nightly            # by hand; or a command after --
arugula runs spec ls                         # with their schedules and rules
arugula runs list --state failed             # newest first
arugula runs show run_…                      # its timeline and artifacts
arugula runs logs run_… --follow             # its output, until it ends
```

Any member reads a project's runs: `GET /api/projects/{project}/runs`,
`GET /api/runs/{run}` (with `since` and `wait`, it answers once the run
has a later event or has ended, waiting at most 15 seconds) and
`GET /api/runs/{run}/log` (`crates/platform/src/triggers/read.rs`). `logs`
reads the platform's durable copy of the log where it keeps one (M213 fills
in `run_log::page`). Until then it reads the run's pane on its hosted
machine end to end, as `arugula tail` reads a pane, while the run goes
and after. It's `arugula runs start`, not `arugula run`, which runs a
command in a pane here. An agent (`CLAUDECODE` or `AI_AGENT` set) reads
runs through the CLI but can't start or change them there: the CLI acts
as its person.

**The runs page (M215).** The platform page has a project's runs at
`/p/<project>/runs`, narrowed by state, by how they started (`trigger`:
`manual`, `schedule`, `event`) and by run spec (`spec`), which the list
route takes too. Owners and editors on `ARUGULA_HOSTED_ACCOUNTS` start a
run there (a spec, or a command) and add and remove specs, schedules and
rules. A run's page (`/p/<project>/runs/<run>`, or `/runs/<run>` from a
push) shows who or what started it, the exact command its operation runs
and the operation's hash, its durable log (M213: from its tail, Earlier a
page at a time, and followed every few seconds while the run goes), its
artifacts (each through M213's signed download), its timeline, and its
thread: an object
room on the run's id, for its project's members (`chat::objects`). While a
run goes, the page waits on `GET /api/runs/{run}` for its next event, and
links its pane on control's page, where its machine is: the platform page
has no machines. A run that fails is an inbox entry (`run_failed`) for
whoever started it (the person who made its schedule or rule, for one of
theirs) and the project's owners, with one push each; a tap opens the run.

**Whose key a hosted agent runs on (M185, #1246).** Keys never enter the
box: E2B's egress proxy sets the key on the box's requests to
`api.anthropic.com` and `api.openai.com`, over the placeholder the agents
hold (`ANTHROPIC_API_KEY=arugula-brokered`). A project brings its own key,
one per provider: its owners set it, the platform keeps it sealed
(`ARUGULA_CLOUD_SECRET_KEY`) and shows it as set or not, never read back,
and writes it into each of the project's boxes' egress rules (at create and
rebuild, on a wake, at a turn's start, and on every running box when it's
set or cleared). E2B's proxy holds it there; E2B's API answers a box's rules
back to whoever holds the platform's E2B key. Without one, a workspace made
by an account on the beta allowlist (`ARUGULA_CLOUD_ACCOUNTS`) runs on the
platform's key, for the providers `ARUGULA_HOSTED_PLATFORM_KEYS` lists: a
dedicated key with a hard monthly spend limit at the provider, kept as an
E2B secret, with per-account caps on agent turns for fairness. Before each
agent turn the box's daemon asks (`POST /api/hosted/turns`, signed as its
hosted machine; the hub hands it to the platform), which brings the box's
rules up to date and says whose key pays, or why the turn can't run: no key
(with how to add one) or a cap (with which). The caps bound agent turns
only: a member who runs `claude` in a terminal on an allowlisted account's
box uses the platform's key too, within its spend limit
(`crates/platform/src/hosted/inference.rs`).

A project's Anthropic key may instead be a Claude subscription's token,
from `claude setup-token` (`sk-ant-oat…`): the boxes' Claude Code then runs
on that subscription, within its plan's usage limits, and not on API
billing. The proxy sets it as a bearer (`Authorization`) rather than
`x-api-key`, and each turn's answer says so (`"auth": "subscription"`), so
the box's Claude Code holds the placeholder as `CLAUDE_CODE_OAUTH_TOKEN`
instead of `ANTHROPIC_API_KEY`, with no API key beside it. An agent that
started holding the other kind starts again on its session before its
prompt goes. The token belongs to whoever made it: everyone who runs
Claude in the project's workspaces spends that person's subscription.

```
arugula cloud key set anthropic --project acme   # reads the key (or token) from stdin
arugula cloud key ls --project acme
arugula cloud key clear anthropic --project acme
```

**The inference broker (M231, Arugula #29).** Claude through the platform
for a project's members and for registered clients (studio's lobby), with
the key set on the way out and never handed to the caller ("The Claude
credential broker and its metering move to the platform"). Point Anthropic's
SDK or Claude Code at
`ANTHROPIC_BASE_URL=<control>/api/projects/<project>/inference/anthropic`:
`POST /v1/messages` (streamed or not), `/v1/messages/count_tokens` and
`GET /v1/models` go to Anthropic, anything else is a 404. Only allowlisted
headers go each way (out: `content-type`, `accept`, `anthropic-version`,
`user-agent` and the betas; back: `content-type`, `request-id` and the
retry headers), and a redirect isn't followed.

- **Who.** A member calls as themselves (a session or a CLI device). A
  registered client sends its credential as a bearer and names the account
  (`arugula-account`, a member of the project) and, if its policy lets it,
  a sponsor (`arugula-sponsor`) and a visitor (`arugula-visitor`). **A
  client acts only for accounts that let it**: each account it names turns
  it on in its settings on the platform page (off until then), and a named
  sponsor or visitor who isn't an account, or hasn't let it, is refused.
- **Whose key pays,** in the lobby's order: a visitor's own key, a
  sponsor's own key, the project's own key (never for a viewer), the
  account's own key (its personal project's: `arugula cloud key set
  anthropic --project personal`), then the platform's key
  (`ARUGULA_INFERENCE_ANTHROPIC_KEY`) under a monthly cap per account (the
  client's for any of its accounts, else `ARUGULA_INFERENCE_MONTHLY_USD`
  for accounts on `ARUGULA_CLOUD_ACCOUNTS`) and at most
  `ARUGULA_INFERENCE_CONCURRENT` of its calls at once, else a refusal with
  how to add a key. On the platform's key only the betas
  `ARUGULA_INFERENCE_PLATFORM_BETAS` names are sent, and the long-context
  ones are refused.
- **Refusals** are 403s (Claude Code shows them at once; too many calls at
  once is a 429, which it retries) in Anthropic's error shape, with
  `arugula.reason` (`no_key`, `cap` with `resets`, `busy`, `consent`,
  `payer_unknown`, `payers`, `beta`, `key_refused`) for a client; the
  answer's `arugula-pays` header says whose key paid.
- **Metering.** Each message is reserved before it goes out (its input as
  the dearest cache write, and all of `max_tokens`), in the same
  transaction as the cap check, under the account's lock. The call runs in
  a task that outlives the request, reads the answer as it streams, and
  settles at list prices (the lobby's table, studio-058, web searches
  included) only once the final usage arrives; a call that ends without it
  (the caller went away, the answer was cut short) keeps its reservation,
  and an error answer costs nothing. Calls are kept per project and per
  account (`inference_calls`, 90 days).
- **Where it shows.** The project's settings page shows what its calls
  cost this month (`GET /api/projects/{project}/inference/spend`), with its
  hosted agent turns beside them (their calls go through E2B's proxy, which
  doesn't see their usage, so they're counted, not priced). The account's
  settings show which clients it lets act for it, and what was spent on its
  own key, by whom (`GET /api/inference/spend/paid`). `GET
  /api/inference/spend` is an account's month on the platform's key and its
  cap, as the lobby showed it (`crates/platform/src/hosted/broker/`).

**studio as a client (M236, Arugula #34).** studio's lobby is a full client
of the platform ("studio is a full client: Sign in with Arugula, apps by
project"): people sign in to it with Arugula (the hub's issuer, M228, and
account links by numeric GitHub id, M229), an app's owners are its
project's members, its egress secrets' values can come from the project's
own secrets, and its boxes' Claude calls go through the broker above. A
registered client (the broker's, by its credential) has routes of its own
under `/api/client/`, the only other platform routes a bearer reaches
(`crates/platform/src/clients.rs`):

- **For an account it names** (`arugula-account`), only once that account
  lets it see its projects (a switch beside the product under Settings,
  Linked products, on the platform page, off until then; unlinking the
  product there ends it, and its Claude switch too), **and only with that
  person's own access token from the hub** (`arugula-access-token`: M228's,
  issued to that client, minutes long, ending with the person's hub
  session; the hub answers whose it is, `HubPort::client_account`), so the
  credential alone never acts for anyone who isn't there. It reads the
  account's projects, its role and their members (`GET
  /api/client/projects`), its apps in them with the account's role (`GET
  /api/client/apps`), one app with the role read fresh (`GET
  /api/client/apps/{app}`: 404 `no_app`, 403 `not_member`), keeps an app
  in a project the account edits or owns (`PUT /api/client/apps/{app}`,
  `personal` for its own; a move needs that of the project it leaves too,
  and ends every secret grant the app had), and asks, for an owner of the
  app's project, to send one of the project's own secrets for that app to
  one host (`PUT /api/client/apps/{app}/secrets/{name}` `{host}`).
  Viewers see an app; editors and owners open it.
- **With no account,** it forgets an app it retired (`DELETE`), and reads
  the value of one of the app's project's own secrets
  (`GET /api/client/apps/{app}/secrets/{name}?host=`), **only under a
  project owner's grant for that client, that app and that host**, and
  not once an agent changed the value since the grant (an owner allows it
  again). Every read is counted on the grant and kept as an event (90
  days); reads are limited to 6,000 an hour per client and 600 per secret,
  and an hour of 120 reads of one secret is pushed to the project's owners
  (and logged, `ops="client-secret-reads"`). Nothing logs a value.
- **Project owners** set and clear the project's own secrets on its
  settings page (agents set them with `set_secret`, M230), see each
  client's asks beside each secret (which app, which host), allow or stop
  each, and see the reads and whether the value changed since it was
  allowed, and by whom; clearing a secret ends its grants. A project's
  members see the clients' apps in it there too.

A link is never authorization: the client acts for an account only by that
account's own switch, and reads a secret only by its owner's grant.

**Agents on hosted workspaces (M184, #1245).** A hosted workspace's agent
blocks are its machine's, as on a laptop: + Agent on the platform page's
workspace (`/p/:project/workspaces/hw_…`) opens Start an agent on control's
page with its machine picked (`#machine=<id>&agent`), and everyone in the
project sees the same blocks. An agent on one of a person's own machines
drives it over MCP: `list kind cloud_workspaces` lists hosted workspaces
first (`kind: hosted`), `cloud` and `read_cloud` take their `hw_…` ids
(a tab is an agent block, by pane number; `diff`, `git_status`, `files`
and `file` read on its machine), and `run`, `start_agent`, `prompt_agent`,
`agent_respond`, `send_input`, `read_output`, `read_file`, `wait`, `show`,
`close` and `list` take `cloud_workspace` to work on its machine. The
person's machine reaches it through control's relay, only with a live
`cloud.drive` grant, and the hosted machine lets in the project's owners'
and editors' machines as their person (a viewer's reach its agents only):
`crates/daemon/src/mcp/hosted.rs`. When an agent block there closes, the
machine archives its transcript to the platform, read only under the
project (`GET /api/projects/{project}/transcripts`).

**Public previews (M187, #1248).** A dev server in a hosted workspace shows
to the project's people in a browser block (M186). One of its owners or
editors can also make a port public: it gets an address of its own,
`https://<id>.arugula.site` (`ARUGULA_PREVIEW_DOMAIN`), that anyone opens
signed out, for 7 days unless renewed, and the workspace shows who made it
public. Control serves it through the relay and sees its traffic, which is
fine because it's public; members' previews stay end to end. A paused box
isn't woken for a stranger: the page says it's asleep. Turning a preview
off takes it down within a minute (docs/control-e2e.md, "Public
previews").

```
arugula cloud preview public dev 5173     # prints its public address
arugula cloud preview ls dev
arugula cloud preview renew dev 5173      # 7 more days
arugula cloud preview off dev 5173
```

No page lists a workspace's ports yet: the CLI is the way in.

**Bodies and comments (M166, #1212).** The platform keeps a linked repo's
PRs and issues as metadata. Opening one (`GET
/api/forge/items/{id}/content`) reads its body, comments, reviews' text
and review comments from GitHub, after the same viewer check as every
read, with a read-only token minted for the reader. They're kept about 3
minutes in memory on that machine, never in the database, and a webhook
for the item drops them. A project can mirror a repo's content instead:
on its settings page, or `PATCH /api/projects/{project}/repos/{owner}/{name}`
with `{"mirror_content": true}`. Turning it on takes what linking takes (a
member who maintains the repo on GitHub). Then webhooks and the poller
keep the bodies and comments in `forge_content`, reads come from there,
and the project's search (`q`) finds their text. Turning it off (any member
who may change links) deletes them at once. Unlinking, or the App being
uninstalled, purges them within a day.

**Forge operations (M169, #1215).** A linked repo's PRs and issues are read
and drafted on without a block, or a machine awake: one route, `POST
/api/forge/ops`, takes a typed operation by name and version (`{"op":
"pr.get", "v": 1, "repo": "acme/app", "number": 7}`) and answers `{"op",
"v", "result"}` (`arugula_control_wire::forge_ops`). Each operation has its
own version; one this control doesn't speak is a 400 that says which it
does. Reads (`pr.get`, `issue.get`, `item.list`, `checks.get`) and drafts
(`draft.create`, `draft.list`, `draft.get`, `draft.discard`) go through the
same viewer check as the routes above (M165), for a person or a machine with
a live `cloud.drive` grant (for its owner). Sends (`draft.send`,
`pr.comment`, `pr.review`, `issue.create`, `issue.comment`) are a person's
only: a machine is refused (403, `reason: "agents_draft"`). A person's goes
out as them through one of their machines (M168, below): a terminal has no
browser for GitHub's authorization. A comment, review or issue that isn't a
draft yet is made one first, so one that can't go now waits as a draft, and
the refusal names it.
The inbox (M171): `inbox.list` reads the reader's (a person's, or a
machine's for its owner), and `inbox.mark` marks an item's entries seen or
done, a person's only (a machine is refused, 403).

**The inbox (M171, #1217).** The platform works out each member's inbox
from the mirror's rows, by a forge block's attention rules: a review
requested from you, your PR's checks failed, changes requested on it, a
mention (only where the project mirrors content), an issue assigned to
you, your PR merged or approved and green (your issue closed), and a draft
waiting for you. A team's review request isn't one: GitHub tells the
platform nobody's teams. Only members the viewer check lets read the item
get an entry, or a push. A reason newly holding pushes once, through the
hub. The platform page shows it at `/inbox`, grouped by reason, with Seen
and Done; `GET /api/inbox` and `POST /api/inbox/{item}` are its routes.

**Mentions and assignments (M202, #1273)** on the platform's own objects
join it (`crates/platform/src/inbox.rs`). Someone added to a native issue's
assignees, or named by an `@` in its body or its thread, gets an entry on
the issue (`assigned`, `mentioned`); an `@` in the thread on a mirrored PR
or issue is a `thread_mention` on the item, beside its GitHub reasons.
Only those who see the object are mentioned: its project's members, and
for a private repo's item those GitHub lets read it (M165). Not for what
you did yourself, and never a machine's agents (they read mentions over
MCP, M198). Each event pushes each person once, with the inbox's limits:
a mention in an object's thread is the inbox's push, not chat's too, and
not when the thread is open or muted. A tap opens the issue on the
platform page (`/issues/<id>`, at the message for a thread's mention) or
the item's thread. The entry stays until it's called done; a new mention
brings it back. Native issues' entries list only for a person who may
read the issue's project now, and never for a machine's agents
(`inbox.list`), which reach issues through their own grants.

Agents use them through MCP: `forge_read` (kinds `pr`, `issue`, `items`,
`checks`, `inbox`, `drafts`, `draft`) and `forge_draft` (kinds `comment`, `review`,
`issue`, `discard`). MCP has no send. `read_forge` on a block of a linked
repo reads the platform's item too (`platform` in its answer), and `draft`
on one keeps the draft on the platform (M170).

**Pins (M172, #1218).** A PR or issue block on a repo linked for the
machine's owner is a pin onto the platform's item (`pin: {kind,
global_id}` in its config and state, kept in `layout.json`): the daemon
reads the item, its checks and its events from the platform, at most every
10 seconds while it's drawn and at once on a poke, and makes no GitHub call
for it. It looks as a block read from GitHub does, less the description
and comments' text, which the mirror doesn't keep. Unlinked repos, Forgejo,
GitLab and machines outside control read as before. The platform says it's
unlinked: the block reads GitHub again. The platform can't be reached: the
block shows what it said last for 10 minutes, then reads GitHub until it
answers (after a restart, at once: a block is never blank). A person's
comment, review or merge from a pinned block still goes out with their `gh`
until sends are the platform's (M168). A machine reporting for M171's
shadow period (`ARUGULA_INBOX_SHADOW`) doesn't pin, so its report stays
GitHub's word.

**Native issue pins (M203, #1274).** A project's own issue opens as an
issue block too: `show` kind issue with `iss_…`, or `#N` in the current
project (`project`, else the project a clone's GitHub repo is linked to
when its issues are native, else your personal one outside a clone; in a
clone otherwise, `#N` is still the forge's), and `arugula open iss_…` or
`arugula open '#12'` (the current project, as `arugula issue` finds it).
The block is a pin, `pin: {kind: "issue", global_id}` in `layout.json`,
and it draws the platform page's issue view. On control's page it reads
and writes as the person looking, with its thread, and updates live: a
change to an issue reaches the pages showing its thread as an `object`
event on chat's stream. The daemon reads it too, as the machine for its
owner, through M200's `GET /api/daemon/issues/{id}`, so only with the
machine's `threads.read` grant over the project (what it reads, its
agents read: `read_forge` on the block is M200's read). It reads again
every minute while the block is drawn, and at once when a page showing it
hears it changed. The platform can't be reached: the block shows what it
said last, and says so. Without a grant, it says how to give one, and
forgets what it read (its title too). What the daemon read is the owner's:
the block's state, its text and the pane list, which everyone who sees the
pane gets, say only `#N`, and the issue itself is the owner's
`native_issue` call. A teammate or a guest looking at the pane reads it as
themselves on control's page, or sees `#N` and that they can't read it
there. Only the owner opens one.

```
arugula pr view acme/app#7                # no block; also `issue view`
arugula pr list acme/app --state open     # or --project; also `issue list`
arugula pr checks acme/app#7
arugula pr review acme/app#7 approve      # sends as you (M168); --draft keeps a draft
arugula pr comment acme/app#7 "lgtm"      # likewise `issue comment acme/app#2 ...`
arugula issue create acme/app -t "Title" -b "Body"
arugula draft ls                          # your waiting drafts; or OWNER/REPO[#N], --project
arugula draft show dft_…                  # discard, send
arugula forge inbox                       # what wants you; --all with what's done
arugula forge done acme/app#7             # seen likewise; --reason, --undo
```

Run by an agent (`CLAUDECODE` or `AI_AGENT` set), the CLI's comment,
review and new issue are drafts, and `draft send` is refused. `%N` instead
of `OWNER/REPO#N` still writes to that block.

**Sending as the person (M168, #1214; aru-085).** A draft on a linked repo
goes out on GitHub as its person, never as `arugula[bot]`, so a review
counts as theirs. Only its own person sends it (a person, or their CLI
device); a machine is refused whatever grants it holds, so an agent's draft
waits for its person ("Agents draft, people send", aru-041). A send holds
the draft for up to two minutes (`sending` on the wire), so the phone and a
block's card at once never post it twice. Two ways, then a clear refusal:
1. **A one-use user token.** The platform page's *Send* asks `POST
   /api/forge/drafts/{id}/send` with `via: "browser"`, and opens the page
   it answers: GitHub's authorization of the App, run by the hub (it owns
   the App's OAuth client). It's silent once the person has authorized the
   App, which signing in with GitHub did. GitHub comes back to the hub's
   own `/auth/github/callback` with a state `send.<nonce>` (random, kept in
   the hub's memory ten minutes, good once, for that account's session on
   control's site). The hub swaps the code for a user token and hands it to
   the platform over the signed internal API (`PlatformPort::send_draft`).
   The platform logs the mint (`github_token_mints`, kind `user`, never the
   token), checks `GET /user` is the account's own GitHub id, and makes the
   one call: the comment, review or issue. The hub then revokes the token
   (`DELETE /applications/{client_id}/token`) and sends the browser back to
   the platform page with `?draft_send=<id>`, which shows what became of it.
   The token is never stored, logged, cached or put in a URL. No App
   setting changes: it's the callback sign-in already uses.
2. **Their machine.** With no browser hop (the CLI, the hub without the
   App's client, GitHub declining, or a token that may not write there),
   the platform asks the person's own machines on control's relay, the one
   that drafted it first, to send it with their own `gh` login: the typed
   op `forge.send` (`arugula_control_wire::forge::DaemonOp`), relayed by
   the hub (`HubPort::daemon_op`). A daemon says which ops it knows when it
   connects; one too old to say is skipped, as is one without a `gh` login,
   for the next. A daemon that doesn't answer within 20 s may have sent it,
   so the draft is `failed` (to check and send again), not tried elsewhere.
3. **Otherwise** it's refused (409, `reason: "no_way"`): "Open this on a
   device to send, or wake a machine." The draft waits, saying so. GitHub
   refusing it leaves it `failed` (502, `reason: "github_refused"`), to send
   again.

A block's card on a linked repo sends a platform draft from its machine
with the owner's login, as its own drafts go, and tells the platform
(`POST /api/forge/drafts/{id}/machine`: `start`, then `sent` or `failed`).

**Agents and deploys (M218).** `deploys.plan` over a project lets a
machine's agents plan its deploys and ask its owners to approve them:

```
arugula grants add geek deploys.plan --project acme
```

It's off until the owner gives it, and it's a switch of its own per machine
and project under Settings, Agents and deploys. Asks are limited per
project. No grant approves a deploy. An owner approves on their phone or
in a browser: the device's signature over the deploy's operation, and a
passkey's assertion over the approval, which the hub checks.

**Environments and planning a deploy (M252).** All of it is on the
platform page, and planning is in the CLI and MCP too:

- **Make an environment** (owners): the project's settings, Environments,
  New environment. Pick a driver; the page asks for that driver's settings
  (required ones marked, defaults shown) and the secret names it may hold.
  A secret the platform mints for each run is shown, not offered.
  `GET /api/projects/{project}/drivers` is what the page reads.
- **Set its secrets** (owners, in a browser only): each secret the
  environment holds has a password box under it. A value is sent once,
  cleared from the page, sealed by the platform, and never shown or read
  back. Set it again to replace it. There's no CLI or MCP for secrets.
- **Plan and ask** (owners and editors): Deploy… on the settings page (or
  on an environment) picks the environment, a commit by its full 40-hex id
  and the repo it's in (the project's linked repos are offered), and
  whether it's a roll back. Plan shows the plan's text, which is what an
  owner approves. Ask for approval sends each owner a card.
- **A project's history** (any member): each environment on the settings
  page lists its latest deploys, whatever became of them (state, commit, a
  roll back's mark, when), each opening its own page, with older ones a
  click away. `GET /api/projects/{project}/deploys` (`environment` by id
  or name, `limit`, and `before` a deploy, for the page after `next`)
  answers them newest first.
- **The CLI**, as your person:

  ```
  arugula deploys envs --project acme
  arugula deploys ls --project acme [--env production]
  arugula deploys plan --project acme --env production --commit <sha> --repo acme/web [--roll-back]
  arugula deploys request dep_…
  arugula deploys show dep_…
  ```

  An agent's shell (`CLAUDECODE` or `AI_AGENT` set) may run `envs`, `ls`
  and `show` only. There's no `approve` and no secrets.
- **An agent**, with its machine's `deploys.plan` grant over the project:
  MCP `plan_deploy` (`project`, `environment` by name or id, `commit`,
  `repo`, `roll_back`) plans and asks in one call, and `read_deploy`
  (`id`) reads its plan, state and log. Without the grant the platform
  refuses and says how to give it.

An environment is named by its id (`env_…`) or its name in the project.
Approving stays the owner's, on their phone or in a browser, with a
passkey: the deploy's card opens control's page for it.

**Applying a deploy (M219).** An approval queues the deploy's apply. Before
anything runs, the platform checks the approval's expiry, the approving
device and the approver's owner role again.

Each deploy gets a fresh box: a hosted workspace made for that one deploy.
Nobody can list it, open it or reach it, the project's owners included. It
doesn't count against the project's workspaces, and its usage is metered
apart. Its daemon gives up root before it runs anything (only the run user
stays reachable with sudo). It takes its one deploy, and stops after.
The box sends its run's log (the pane's output, redacted) before it
reports the end. The platform deletes the box once that log is in (or
after two minutes), whatever happened: a failure, a refusal, a box that
never came up, or a crash on the platform part way. A reaper deletes any
deploy box past its longest life.

The box checks the approval itself, against the owners it pinned when it
enrolled: an owner's browser signed exactly this operation, it hasn't
expired, a passkey confirmed it, and the plan is its driver's. Only a
deploy's own box applies a deploy, and only that one. The deploy starts
only when its run starts, before the approval expires, and that start uses
the approval up: it never starts the deploy again or goes to another run.

Secrets for a deploy are set per environment, by an owner, from a browser
(not a CLI): the environment's password boxes on the platform page, which
call

```
POST /api/environments/{id}/secrets/{name} {"value": "..."}
```

Each driver takes only the secret names it declares (the fake driver
`APP_TOKEN`; chant→Fly `FLY_API_TOKEN` and, instead of a minted one,
`GITHUB_TOKEN`; apps-platform `APPS_PLATFORM_TOKEN`; arugula-release and
planter's drivers none,
its tokens are minted, see
[control-ops.md](control-ops.md#releasing-arugula-as-a-deploy-m221)). Names that change how a shell or a tool behaves
(`PATH`, `IFS`, `BASH_ENV`, `LD_*`, `NODE_*`, `GIT_*`, `NPM_CONFIG_*`,
`SSL_*`, `*_PROXY`, `DEPLOY_*` and the like) are refused everywhere, and a
value can't hold a NUL. Values are sealed with the project's key and never
read back. The box gets them once, while its run applies the deploy, and
hands them to the driver on a pipe. They're never written to the box's
disk. A GitHub token a driver needs is minted for that run alone, for one
repository, and revoked when the run ends, by the box itself and by the
platform. A token that writes (chant→Fly's ledger, apps-platform's list)
asks for Contents: write, so the App's installation there must grant it:
a deploy is refused when it's planned if it doesn't. A token for the
deploy's own repository, read or write, is minted only while that
repository is linked to the deploy's project, and for it by GitHub's id.
That's checked when the deploy is planned and again when the token is
minted, so a repository unlinked after the approval gets no token.

The driver runs beside the run's pane, not in it. Its output reaches the
pane with each secret replaced by `[redacted NAME]`, its exact bytes and
trimmed. **That's best effort:** a driver that prints a secret encoded or
split differently gets past it. Drivers must never print their secrets.

A driver that fails marks the deploy failed, and the project's owners get a
push with the run's log. A box that refuses an approval (something forged
or stale got past the platform) cancels the deploy before anything runs,
and the owners get a push saying why.

**The apps-platform driver (M222).** It deploys an app to home-cloud's app
platform. Its source is a full commit of one of the list's organization's
repositories (`arugula-salad/<app>`, not the list itself), and the plan names
the list repository, the pointer's path (`apps/<app>.yaml`) and the commit.
The box gives the driver the deploy it checked on its command line, and the
driver recomputes the approved plan from it.

Apply clones the list and pins the pointer's `GitRepository` at that commit
(nothing else in the file changes; a new app gets a pointer as planter writes
it). It commits that one file and pushes, and Flux applies it. It refuses:

- a commit that isn't on the app repository's default branch (GitHub serves a
  fork's commits by their sha too);
- a deploy whose id is in the list's history already (a replay);
- a new app whose name another of the list's objects holds, or overlaps by a
  dash-prefix;
- a repository or `app.yaml` over its size bound.

The token writes that one repository only: an installation token of the
platform's GitHub App, `contents: write` on the list alone, minted for the
run. An environment can hold a fine-grained token of its own as
`APPS_PLATFORM_TOKEN` instead. A second, read-only token (`SOURCE_TOKEN`, a
name environments can't use) reads the app's repository. Both reach git as
headers in its environment, never on a command line or on disk. The box
revokes both at GitHub right after the push; the platform revokes them again
when the run ends.

The run succeeds once the app's public address answers its health path, three
times in a row, within ten minutes. An app whose answer reports its revision
(JSON `revision`, `commit`, `version` or `sha`, or an `X-Revision` header) is
held to it: it must be the commit's, or the image that commit pins. **An app
that reports none is ready when it answers,** and an old version still
serving answers too, so for it a failed rollout can look ready.

Rolling back is a deploy of the last good commit: when the pointer's last
commit is this driver's and it pinned that commit before, it's a revert
commit of it.

**planter's drivers (M233).** Only in the projects `PLANTER_PROJECTS`
names, and for accounts hosted workspaces allow. planter's four ways to put
an app on home-cloud's app platform are drivers, writing the same repositories planter
wrote (`arugula-salad/planter-apps`, `arugula-salad/shipped`), each deploy
approved on an owner's device like any other. They render with planter's own
`render.mjs` and `prod-scaffold.mjs`, vendored byte for byte in
`crates/e2e/src/planter/` and run with node on the box; the plan names their
digest. Their tokens are installation tokens of planter's GitHub App (the
deploy App), minted for the run, one repository each, never the platform's
App ([control-ops.md](control-ops.md#planters-deploys-m233)).

- **`planter-prod`** (planter's Prod): the pull request that gives an app's
  repository (`arugula-salad/<app>`, linked to the project, a commit on its
  default branch) planter's scaffold: a Dockerfile, the build-and-pin
  workflow, `app.yaml` and its `k8s/`. Settings: `app_dir` (`app`), and
  `health`, `port_env`, `data_env`, `secrets`, `postgres`, `metrics` and
  `volume` where the app's own files don't say (planter read them through
  chant; here the plan names them). A person merges it.
- **`planter-go-live`** (Go live): apps-platform's apply, through planter's
  App: the pointer pinned at the approved commit. A new pointer asks for the
  custom domains its setting `domains` lists (space separated, in the plan),
  as planter's did, and only when the app's `app.yaml` routes exactly
  those. Rolls back.
- **`planter-ship`** (ship): an agent's source, a gzipped tar of regular
  files with a Dockerfile at its root (at most 2 MiB, read on the box with
  running caps, never extracted), into `shipped`'s `<app>/` with
  `app.yaml` for loam and its render, at `https://<app>.run.inevitable.fyi`;
  the pointer the first time. Settings: `app` (required), `port`, `health`,
  `port_env`, `secrets`. Its source is an artifact the platform keeps
  (`sha256:<hex>`, where `platform`), sent by an agent with the hosted MCP's
  `plan_deploy` and `files`; the box fetches it and checks its digest. A
  shipped app's name is one project's (the first environment to name it),
  it's never an arugula-salad repository's name (as planter's App sees
  them), and a ship never goes over an app another project (or planter)
    shipped last. Its `secrets` are loaded from Infisical's `/<app>`, so a
  ship asks for them only under a name nobody else ever shipped or pointed
  at. The platform keeps a source only once its deploy is planned, drops
  it when no deploy waiting or running names it, and keeps at most 200 MiB
  a project.
- **`planter-promote`** (promote): the pull request moving a shipped app from
  loam to the cluster, from a commit of `shipped`. Setting: `app`. A person
  merges it.

```
POST /api/projects/{project}/environments
{"name": "hello", "driver": "planter-ship", "settings": {"app": "hello", "port": "8080"}}
```

**What a compromised platform can do.** The platform holds the master key,
so it can open a project's secrets itself, and it chooses where a deploy's
box runs. The box's own checks guard against a platform that's only partly
compromised (one that can send work but not forge an owner's device
signature):

- it won't apply what no owner's browser signed;
- it won't apply another driver's plan, or secrets its driver doesn't take;
- it won't apply after the approval expired by its own clock.

Each deploy's box is new, so a box can't tell on its own that a deploy was
applied elsewhere. Against a replay, the platform uses an approval up when
its deploy starts, and each driver's target refuses a deploy id it applied
(chant→Fly by chant's ledger, apps-platform by the `Arugula-Deploy: <id>`
trailer).

The box can't check the passkey's signature yet: only the hub keeps
passkeys' public keys. It checks the passkey's challenge and that it
verified the person.

**The chant→Fly driver (M220).** An environment with the `chant-fly`
driver releases a commit with the repository's own chant op, onto Fly. An
owner makes it with its settings and its secrets' names:

```
POST /api/projects/{project}/environments
{"name": "production", "driver": "chant-fly", "secret_names": ["FLY_API_TOKEN"],
 "settings": {"fly_app": "acme-web", "chant_env": "github", "chant_dir": "ops"}}
```

Settings (each starts with a letter or a digit):
- `fly_app` and `chant_env` are required. `chant_env` is the environment
  chant's release ledger records the release under.
- `chant_dir` (the chant member, from the repository's root) defaults to
  `.`, and `chant_op` to `release`. `fly_config` (a `fly.toml` in the
  repository) is used by a roll back.
- `chant_gates`: the only gates the driver approves, comma separated.
  Left out, it approves each gate of the op it stops at, never another
  op's.
- `npm_scripts: "true"` lets `npm ci` run install scripts. Without it,
  `npm ci --ignore-scripts`.
- `chant_github: "true"` gives the op `GITHUB_TOKEN` too, for an op that
  pushes, such as a release that tags. Without it, chant can't reach
  GitHub, so its own git reads and writes its ledger through a stand-in for
  origin on the box. The stand-in holds `chant/lifecycle` as origin had it
  (none on a first deploy) and takes that branch only. The driver pushes
  it.
- `chant_digest`: `plan` (the default) or `image`. With `image`, the
  release record's digest must be the image Fly runs. With `plan`, Fly
  must run a different image than before.

Secrets:
- `FLY_API_TOKEN` must be a deploy token for the one app that expires
  within 30 days: `fly tokens create deploy -a <app> -x 720h`. The box
  reads its caveats with `flyctl tokens debug` and refuses an org token or
  one that lives longer. A deploy token names its app inside an
  `IfPresent` caveat; that counts only when the caveat's `else` is read at
  most and it holds nothing but `Apps`, `FeatureSet` and
  `ValidityWindow`. The expiry is read from the token's top-level windows.
  Nothing of the repository's runs before that check.
- `GITHUB_TOKEN` pushes chant's ledger (`chant/lifecycle`) to origin. The
  platform mints it for each run through its GitHub App: an installation
  token that writes the deploy's repository's contents and nothing else,
  revoked when the run ends. The App must be installed on the repository
  with Contents: write. A deploy is refused when it's planned if it isn't,
  so an owner never approves one that can't run. An environment that holds
  a `GITHUB_TOKEN` of its own gets that instead, and nothing is minted. The
  driver checks it can push before anything runs. Only the driver's own
  `git fetch`, `push` and `ls-remote` use it, through a credential helper
  for `https://github.com`.

**The repository's chant op runs with `FLY_API_TOKEN`** in its environment:
that's how it deploys. `npm ci` and `chant components release` get neither
token. The plan says all of this, and the deploy's page shows it
(`plan_text`). The approval dialog checks that the text hashes to the
plan it signs, and says not to approve if it doesn't.

On the box, the driver:
1. checks out the commit by its full id, exactly;
2. runs `npm ci` and `chant run <op> --json` in the chant member;
3. at each gate, runs `chant approve <op> <gate> --approver <account>
   --relayed-by arugula --note "..." --plan <digest>`, then runs the op
   again. The note names the platform's deploy, approval, approving
   device, operation hash and run. The digest is the plan chant's run
   recorded: the `planDigest` of its pending fact in
   `_gates/<op>.jsonl` on `chant/lifecycle`. A gate with no plan digest,
   or one the run's `approve` hint disagrees with, isn't approved, so a
   chant op's gates need a plan.

chant's records then say:
- the gate's resolution: `resolvedBy` is the approver's account id (not
  their email), `relayedBy` is `arugula`, with the note and the plan's
  digest;
- the release: `actor` is `arugula`, `approver` is the account id.

The driver finds the release record this run added for the commit in
`chant_env`, pushes chant's ledger, and checks the app with `flyctl
status`: every started machine's checks must pass, on one image. It says
what it did on fd 3, a pipe only it holds, not on its output. The platform
keeps only a link to that line in the deploy's own repository on GitHub
(`https://github.com/<repo>/blob/<commit>/….jsonl#L<n>`) and an image of
the app's by digest. The run links the entry (its `chant_ledger`, shown on
the run's page), a failed apply's too. The deploy succeeds only if Fly is
healthy, the entry reached origin, and the record is tied to what Fly runs.

**Rolling back** is a new deploy, planned with `"roll_back": true` and the
commit to go back to. That commit must be one the environment ran, and the
image comes from that deploy's log. It's approved like any other deploy.
The box runs `flyctl deploy --app <app> --image <image>` and checks the app
runs that image, healthy. Then `chant components release` records the
release again in chant's ledger, as the approver, and the run links that
entry.

**Agents in conversations (M197).** Three more powers let a machine's
agents into its owner's conversations: `threads.read` and `threads.post`
over a project (a team's channels are its default project's; an object's
thread is its project's) or over the owner's DMs, and `issues.write` over a
project. They're off until the owner gives them, and only over what the
owner reads; a machine's agents never reach more than its owner, so one
whose owner leaves the team is refused there at once (within the platform's
cache delay), grant or not.

```
arugula grants add geek threads.read threads.post --project acme
arugula grants add geek threads.read --dms
arugula grants revoke geek threads.post --project acme
```

or a switch per machine and project (and one for your DMs) under Settings,
Agents in conversations, on the platform page. Through the API it's `POST
/api/grants {"daemon", "power", "scope": {"kind": "project", "id"}}` (or
`{"kind": "account"}` for your DMs). Every call an agent makes asks for the
grant again (`grants::agent_may`), so a revoked one refuses the next call,
with a message naming the machine and how to grant it. What an agent posts
is its owner's, labelled "Jake's agent on geek" wherever it shows, and stays
labelled after a revoke. Every room an agent grant reaches says so to
everyone in it: "Jake's agents on geek can read and post here".

Grants bound what a machine's own signature can do (its MCP calls), not
what a shell on it can do with your CLI login. The CLI's device key lives
on the same machine as its agents, so an agent with a shell there can act
through `arugula` as you, unlabelled, `arugula grants add` included (as it
can `arugula grants restore` for `cloud.drive` today).

**What a CLI login can't do.** Its signature counts only on the routes
routes.toml lists for `cli` (401 `page_only` on the rest). It can't allow
the desktop app's sign-in, add a passkey, enroll or approve a device,
approve a machine's join, make recovery codes, or approve a deploy: those
need control's page in a browser, by its cookie, and adding a passkey or
approving a deploy needs a passkey too. A CLI device approves only CLIs
and machines (no command does today), never a browser. Its key also can't make a team's owner or editor, and the app's can't
make an owner (it makes editors); neither removes a browser or a phone:
daemons and pages refuse those papers unless a browser all the way back to
the account's root signed them. The desktop app's
window is an `app` device: it reaches your machines and approves machines,
CLIs and apps, but not browsers, phones, recovery codes or deploys.
Before #1343 the app's window enrolled as a browser it named ("Arugula app
on …"). A browser such a window approved, both stored and approved by
control before #1343 shipped (2026-10-22 20:18 UTC, by control's own
record), counts again: it can sign for a passkey and approve deploys. The
window itself can't, nor anything it approves now.
[control-e2e.md](control-e2e.md#page-only-actions-need-a-browser-and-a-passkey)
has the details.

**Agents reading and posting (M198).** With those grants, the `arugula`
MCP server's agents on that machine use four tools, each a call to the
platform signed by the machine (the daemon keeps no thread of its own):
`list` kind `threads` (the rooms its grants reach, with the newest message
that mentions it), `read_thread` (a room, or an issue's or PR's thread by
its id, paged by `seq`; `mentions_only` for what mentions it),
`search_threads`, and `post_thread`. The reads are in the permission
snippet's allow list and the post in its ask list; an agent a guest stands
behind is refused. `@` and a machine's name (`@geek`, or `@geek's agent`)
in a room its `threads.read` grant reaches is a mention of its agents
(posting or `issues.write` alone isn't: they couldn't read it), kept as
`machine:<id>` among the message's mentions. It pushes nobody, and nothing
wakes an agent: they read their mentions when they look. `capabilities`
says whether the thread tools are live and what they reach. The routes are
`/api/daemon/chat/rooms`, `/api/daemon/chat/rooms/{id}/messages` (`GET`,
`POST`), `/api/daemon/chat/search`
(`crates/platform/src/chat/agent_threads.rs`), and, for what a message can
carry: `.../files` (`POST` an upload, `GET .../files/{file}`),
`.../messages/{seq}/reactions` (`POST`, `DELETE`) and
`.../messages/{seq}/pin` (`PUT`, `DELETE`). Posting, files, reactions and
pins need `threads.post`, reading needs `threads.read`. `post_thread` takes
`files` (paths the agent may read: at most 10 of 10 MB), or instead of text
`react_to_seq` with `emoji`, or `pin_seq`, each with `remove` to undo;
`read_thread` begins with the room's topic, description and pins and shows
each message's reactions, attachments and pin, and system lines as
`* alice pinned a message`. A reaction or a pin is the owner's, labelled
with the machine, and pushes nobody. [labs.md](labs.md#threads-and-chat)
lists the rest of chat and which of it agents don't get.

**Agents on native issues (M200).** The same machine's agents read and
write a project's native issues over MCP. An issue is `#N` in the current
project (the call's `project`, else the one the agent's clone's GitHub
repo is linked to, else the owner's personal project; a clone of a repo
none of the owner's projects links is refused, not guessed) or its
`iss_…` id. Reads: `read_forge` with `block` `#N` or `iss_…`, and
`forge_read` kind `issue` (the same) and `native_issues` (a list or a
search). Writes: `issue_write`, kinds `create`, `comment`, `edit`,
`label`, `assign`, `close` and `reopen`. They land at once, as the
owner's, labelled with the machine: the owner's role is the ceiling, and
the machine needs `issues.write` to file and change (a change reads first,
with `threads.read`) and `threads.post` to comment. A GitHub issue or PR
still gets a draft a person sends (`draft`, `forge_draft`; aru-041,
aru-085); given a native issue they refuse and point to `issue_write`.
`issue_write` is in the permission snippet's ask list. The routes are
`/api/daemon/issue-home`, `/api/daemon/projects/{project}/issues[/{number}]`
and `/api/daemon/issues/{id}[/events]`
(`crates/platform/src/issues/agent.rs`); a comment is a post to the
issue's thread by its id.

**Agents and runs (M216).** Three more powers let a machine's agents at
a project's runs, each over one project, off until the owner gives it:
`runs.read` (its runs, their timelines and logs), `runs.start` (start one
of its run specs) and `runs.command` (start a command or an instruction of
the agent's own there). A spec is what the project's people wrote down to
run; an agent's own command could be anything, so it's a power of its own.

```
arugula grants add geek runs.read runs.start --project acme
arugula grants add geek runs.command --project acme
```

or two switches per machine and project under Settings, Agents and runs, on
the platform page. A run an agent starts is its owner's start by hand: the
owner must be one of the project's owners or editors on
`ARUGULA_HOSTED_ACCOUNTS`, it's recorded with the machine
(`started_by_agent`), and it runs under the project's live standing grant
like any trigger's, or waits for an approval without one. No power is an
approval of anything: a deploy's is an owner device's (M218). The MCP
tools are `start_run` (a spec, a command or an instruction, on one of the
project's hosted boxes), `read_run` (a run's state, timeline and a page of
its log by byte offset, or a project's latest runs) and `wait` with `run`
(until it ends, then the log's tail). `start_run` is in the permission
snippet's ask list, `read_run` in its allow list, and an agent a guest
stands behind is refused. `capabilities` says whether they're live
(`control.runs`). An agent reads a run's log only once the platform keeps
it (M213): this machine can't read the run's pane on its box, since a
machine reaches another only for its agents (`/api/a2a/`). The routes are
`/api/daemon/runs/reach`, `/api/daemon/projects/{project}/runs` (`GET`,
`POST`) and `/api/daemon/runs/{run}[/log]`
(`crates/platform/src/triggers/agent.rs`).

A read that waits (`since` and `wait`) is woken when its platform machine
moves the run, and looks every 2 seconds for a move another machine made.
At most 8 are open at once per account and per machine (429 past that,
and `wait` backs off), and they're counted per hour apart from other
reads (2000), so long waits don't use up a machine's reads.

**Agents with no machine (M230, platform step 8).** An agent that runs
somewhere without `arugulad` (a hosted service's, redone's) reaches a
person's projects through the platform's own MCP endpoint, `POST /api/mcp`
on control's site, behind the hub's gateway like every platform path ("A
hosted MCP endpoint serves agents with no machine", aru-111, amending
aru-038): machines' agents still use their daemon's.

- **A bearer agent grant token** says who it is. Its owner mints one on
  the platform page only (Settings, Agents with no machine), in a signed-in
  browser: a CLI's login is refused, as for environment secrets, since its
  key is a file an agent on its machine could read. It's over projects they
  read, with powers from the agents' list (`threads.read`, `threads.post`,
  `issues.write`, `deploys.plan`, `runs.read`, `runs.start`, `runs.command`)
  and `secrets.write`. Never `cloud.drive`, and never an approval. The token
  (`aru_agent_…`) is shown once. The platform keeps its SHA-256
  (`agent_tokens`), the browser it was minted from (its User-Agent: the hub
  ties no session to a device), and its powers as grants (`subject_kind`
  `agent`, "Grants, not impersonation"). Every mint pushes the account's
  devices a notice. Its powers are fixed when it's made: a token that should
  do more is another token. At most 10 live per account. A name is plain
  text: no control or Unicode format (Cf) characters, and the page shows it
  inside `<bdi>`.

  ```
  arugula agent-tokens                  # live and revoked, last used
  arugula agent-tokens calls redone     # what its agent did, newest first
  arugula agent-tokens revoke redone    # refused from its next call
  ```

  The API is `GET`/`POST /api/agent-tokens` (`POST` from the page only),
  `POST /api/agent-tokens/{id}/revoke` and `GET
  /api/agent-tokens/{id}/calls`.
- **A project's owners see every token over it,** whoever minted it, and
  revoke its reach there (`GET /api/projects/{project}/agent-tokens`, `POST
  …/agent-tokens/{id}/revoke`; the project's settings page). A token's
  grants over a team's project are revoked when its owner leaves the team
  (with the roster's projection, `db::agent_tokens::revoke_departed`).
- **The same checks as a machine's agents.** Each tool is the route a
  machine's agents call, with the token in the machine's place: its live
  grant over the project, asked on every call (`grants::agent_may`), the
  owner's reach and role as the ceiling, the same rate limits, counted per
  token. What it writes is its owner's, labelled "Jake's agent on redone
  (MCP)" wherever an agent's machine is named, and every room its
  `threads.*` grants reach says so.
- **The tools** keep the daemon's names where they overlap:
  `capabilities`, `list` (kind `threads`), `read_thread`, `post_thread`,
  `search_threads`, `issue_write`, `forge_read` (kinds `issue` and
  `native_issues`), `start_run`, `read_run`, `wait` (a run), `plan_deploy`
  (plans and asks) and `read_deploy`. And `set_secret`: a project's secret
  by name (`secrets.write`, the owner an owner or editor there), sealed
  with the project's key under `project/<NAME>` in `project_secrets`, and
  never read back by any tool; at most 100 a project. The project's owners
  see each by name, with who last set it, through which token, and when
  (`GET /api/projects/{project}/secrets`, the project's settings page). It
  can't set an environment's secrets: those are an owner's, from a browser
  (M219). Nothing reads `project/*` yet. If one ever feeds a deploy or a
  run, a change to it has to make pending approvals stale, as another apply
  does, since an approval is over the plan and not the secrets.
- **It never approves.** No tool approves or denies anything, a call that
  names one is refused and noted (as `approve`), and the approve and deny
  routes take a person's session and their device's signature, never a
  token. The token opens `/api/mcp` and nothing else: no other route reads
  it, and the hub drops a client's `Authorization` on every platform route
  but `/api/mcp` and the inference broker's (`gateway::bearer_route`), in
  process and out.
- **Streamable HTTP, stateless:** one JSON-RPC 2.0 message per `POST`, at
  most 64 KiB. No batches (MCP 2025-06-18 has none), `202` for a
  notification, and a request with no `id` (a tool call sent as a
  notification) is refused and runs nothing. No session and no server
  stream. A missing, unknown or revoked token is `401` with
  `WWW-Authenticate: Bearer`.
- **Limits:** each token's own, and one across all of an account's tokens
  (requests, deploys asked for, runs started).
- **Every call is noted** for its owner (`agent_token_calls`): the tool,
  the project, `ok`, `refused` or `failed`, and what it made or why not,
  never an argument's value. A name that's no tool is answered and not kept.
  The deploy approval's push names the asker as "Jake's agent token
  ‘redone’".

For Claude Code: `claude mcp add --transport http arugula
https://control.arugula.io/api/mcp --header "Authorization: Bearer
aru_agent_…"`. The code is `crates/platform/src/agent_mcp.rs` and
`agent_tokens.rs`.

**redone's agents (M235, platform step 8).** redone's tenants, tokens and
apps join the platform through an operator's import (`arugula-platform
import redone`, docs/control-ops.md): a tenant is a project, found by its
people's numeric GitHub ids; each app is an environment of it; each `rdn_`
token is an agent grant token with `deploys.plan` and `secrets.write` over
it, kept by the hash redone kept, so the token works on `/api/mcp`
unchanged. Its agent gets redone's tools beside the platform's (`whoami`,
`list_apps`, `plan_deploy {app, upload_id}`, `get_plan`, `apply_plan`,
`plan_rollback`, `app_status`, `app_logs`, `set_secret {app, key, value}`,
`list_secrets`) and uploads a source with `PUT /api/mcp/uploads`. Nothing
of it approves: the approver key is gone, and an owner approves each deploy
on their phone. An agent never claims an app's name: a call naming an app
the project has no environment of is refused. An app's secret is the
project's (`app/<app>/<KEY>`), and the platform projects it into the
operator's Infisical when the projection is on
(`crates/platform/src/infisical.rs`). The code is
`crates/platform/src/redone.rs` and `agent_mcp_redone.rs`.
