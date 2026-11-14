# arugulad-control-tools

MCP's tools for what lives in Arugula control, split out of `arugulad-base`
(#232) so an edit here rebuilds this crate and the binary, not the daemon.
Each call is a route on control's platform, signed with this machine's key
and acting as its owner through their grants; nothing is kept here.

- `src/cloud.rs`: Cloud workspaces (`cloud`, `read_cloud`, `list` kinds
  `cloud_workspaces` and `cloud_agents`, #1094);
- `src/forge_ops.rs`: linked repos' PRs and issues with no block open
  (`forge_read`, `forge_draft`, M169);
- `src/repos.rs`: linking repos to projects (`link_repo`, `list` kind
  `linked_repos`);
- `src/threads.rs`: chat's rooms (`read_thread`, `search_threads`,
  `post_thread`, `list` kind `threads`, M198);
- `src/issues.rs`: writing native issues (`issue_write`) and listing them
  (`forge_read` kind `native_issues`, M200);
- `src/boards.rs`: boards (`board_read`, `board_write`);
- `src/runs.rs`: runs (`start_run`, `read_run`, `wait` with `run`, M216);
- `src/deploys.rs`: deploys (`plan_deploy`, `read_deploy`, M252);
- `src/splits.rs`: planning to move work out of a project, which the
  person then moves (`project_split`, Arugula #973);
- `src/agent_sessions.rs`: agent sessions (`session_start`,
  `session_reply`, `session_stop`, `read_session`, `list` kind
  `agent_sessions`, aru-121).

The old names of some arguments and kinds (`session` for an agent
session, `box`, `tab`, `sessions`, `cloud_tabs`, `open_tab`, `close_tab`)
are accepted for good: `arugulad-mcp`'s `ARG_ALIASES` and `KIND_ALIASES`
read them as the new ones (`agent_session`, `cloud_workspace`,
`agent_block`, `agent_sessions`, `cloud_agents`, `open_agent`,
`close_agent`) before a call is routed, and a result carries the old
field beside the new (`cloud_tabs`, `tab`) for older callers (Arugula
#668).

`register()`, which the binary always calls before `arugulad-invite` (whose
tools list after `forge_draft`), puts the tools and kinds into the base's
tool list where they were (`mcp::registry`), so the list and its fixture
are unchanged, and registers what the base asks of them
(`mcp::registry::ControlTools`): whether they reach control from here, for
`capabilities` and the card, and `wait` on a run. The base keeps reading a
native issue (`mcp::issues`), which `read_forge` does too, and refusing one
in `draft`.

The code here still names what it uses as `crate::…`, as it did when it was
the daemon's: `lib.rs` brings those in. Like every library here it's version
0.0.0.
