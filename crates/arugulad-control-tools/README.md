# arugulad-control-tools

MCP's tools for what lives in Arugula control, split out of `arugulad-base`
(#232) so an edit here rebuilds this crate and the binary, not the daemon.
Each call is a route on control's platform, signed with this machine's key
and acting as its owner through their grants; nothing is kept here.

- `src/cloud.rs`: Cloud workspaces (`cloud`, `read_cloud`, `list` kinds
  `cloud_workspaces` and `cloud_tabs`, #1094);
- `src/forge_ops.rs`: linked repos' PRs and issues with no block open
  (`forge_read`, `forge_draft`, M169);
- `src/repos.rs`: linking repos to projects (`link_repo`, `list` kind
  `linked_repos`);
- `src/threads.rs`: conversations (`read_thread`, `search_threads`,
  `post_thread`, `list` kind `threads`, M198);
- `src/issues.rs`: writing native issues (`issue_write`) and listing them
  (`forge_read` kind `native_issues`, M200);
- `src/boards.rs`: boards (`board_read`, `board_write`);
- `src/runs.rs`: runs (`start_run`, `read_run`, `wait` with `run`, M216);
- `src/deploys.rs`: deploys (`plan_deploy`, `read_deploy`, M252);
- `src/agent_sessions.rs`: agent sessions (`session_start`,
  `session_reply`, aru-121).

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
