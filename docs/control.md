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
workspaces through MCP ([managoat.md](managoat.md#cloud-workspaces)).
**A teammate with run rights on a machine can therefore drive its owner's
Cloud workspaces through the agents there.** That was decided, not
overlooked. A daemon can't change a Cloud workspace's settings, delete one,
or reach secrets, the agent library or past tabs; those stay a person's
(`auth::CloudDriver` in `crates/control/src/auth.rs`).
