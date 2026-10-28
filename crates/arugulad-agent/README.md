# arugulad-agent

The agent and chat blocks and the Claude Code conversations on this machine,
split out of `arugulad-base` (#232) so an edit here rebuilds this crate and
the binary, not the daemon:

- `src/lib.rs`: the agent block (M6b), an ACP client: it starts an agent
  server (`claude-agent-acp`, `codex-acp` or any ACP command), turns its
  `session/update`s into a transcript, and asks for permissions and answers
  as cards; and the chat block, the same code as Claude Code with every tool
  off. `link.rs` is the agent server's process and pipes, `chant.rs` a
  chant workspace's member, `background.rs` and `turnwork.rs` the turns a
  background task wakes and the work a turn leaves;
- `src/conversations/`: the Claude Code conversations in Claude Code's
  folders (M33), which an agent block imports and follows;
- `src/ops.rs`: listing and opening them (`ConversationsList`,
  `ConversationOpen`) as `Handle<AgentBlock>`, routed with `op_of`, and
  MCP's `list` kind `conversations` and `show` kind `conversation`.

`register()`, which the binary always calls, adds the block types through
the base's `features` hooks (`Feature::kinds`, handed MCP's link, the
secrets and the rules), the operations and MCP's kinds, and `agent::Agents`:
the open blocks' transcripts, which a hosted box archives, and the
conversations, which the mux asks of. The base keeps `agent/`: the agents
and their adapters, the hosted gate, pictures, the transcript, a worn recipe
and its secrets, and what work sessions and deploys share with the block.
MCP's `Link`, relay and session entry stay the base's, so this crate sits on
the base alone.

Its `labs` feature is the mux's (`Block::wrote_run`), as the base's is; the
binary's `labs` turns it on.

The code here still names what it uses as `crate::…`, as it did when it was
the daemon's: `lib.rs` brings those in. Like every library here it's version
0.0.0.
