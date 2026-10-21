# Arugula control

Using control (signing in, devices, joining machines, teams) is on
**[docs.arugula.io/control](https://docs.arugula.io/control/)**. Operating the
hosted one, running your own (unsupported for now, #662) and testing it are in
[control-ops.md](control-ops.md); the design in [control-e2e.md](control-e2e.md).

**A joined machine acts as its owner in their Cloud workspaces (#1094).**
Control takes a daemon's signature on the Cloud workspace and tab routes
(list, create, show, the machine's files, diff, git status and setup log,
tabs, prompts, interrupts, answers, rebuild and retry, and the agent and
model lists) and scopes it to the daemon's account, as it would that
account's own session. The agents on that machine drive those Cloud
workspaces through MCP ([managoat.md](managoat.md#cloud-workspaces)), and
can put one beside a pane as a Cloud workspace block (#1117), whose reads
and writes go the same way.
**A teammate with run rights on a machine can therefore drive its owner's
Cloud workspaces through the agents there.** That was decided, not
overlooked. A daemon can't change a Cloud workspace's settings, delete one,
or reach secrets, the agent library or past tabs; those stay a person's
(`cloud::CloudDriver` in `crates/platform/src/cloud/mod.rs`).

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
