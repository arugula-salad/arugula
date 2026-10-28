# arugulad-invite

Inviting a person into a session (#233, #234), split out of `arugulad-base`
(#232) so an edit here rebuilds this crate and the binary, not the daemon:

- `src/lib.rs`: sharing a session with someone and telling them, and only
  them (`run`), whom an invite may name (`resolve`), and the teams the
  owner's browser pinned for naming people;
- `src/ops.rs`: its operations, `impl Handle<Invite> for InviteSend` and the
  team pins', routed with `op_of`;
- `src/card.rs`: the invite block, the card an agent's invite waits on until
  the owner sends or declines it;
- `src/tool.rs`: MCP's `invite_person` and `read_invite`, registered after
  `forge_draft` through `mcp::registry`.

`register()`, which the binary always calls, adds them through the base's
`features` hooks. The base keeps what its own API checks (`invite.rs`: the
agent header, the owner-only words) and the `Hook` the card runs an approved
invite through, which Labs' agent catalog block holds too.

The code here still names what it uses as `crate::…`, as it did when it was
the daemon's: `lib.rs` brings those in. Like every library here it's version
0.0.0.
