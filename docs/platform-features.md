# Platform features in Arugula

Gathered 2026-10-07. Arugula already speaks the two protocols that matter most
for agents (ACP and MCP), and A2A is planned. The features people still leave
Arugula for are recurring tasks, the UIs that tools draw in the vendor apps,
and anything that lives only in the vendors' own apps.

## The question

Arugula wants to be the one window where people and agents build together.
Joe Engineer already uses vendor features: recurring tasks and artifacts in
Claude, plugins in ChatGPT, plan mode in Codex. Each time he needs one, he
switches apps. This page lists which of those features Arugula already brings
in, which are planned, and which no doc or issue covers yet.

There are three ways in, and they set what is possible:

1. **Open protocols** any app can speak: ACP (editor to agent), MCP (agent to
   tools, with MCP Apps for tool UIs), A2A (agent to agent), and `SKILL.md`
   skills.
2. **Vendor agents we run ourselves:** Claude Code and the Agent SDK, Codex
   through its app-server, Gemini CLI.
3. **Vendor app only, with no API:** Cowork scheduled tasks, Claude Design,
   claude.ai Projects and memory, chats in the Claude and ChatGPT apps. The
   only options here are to rebuild the habit in Arugula or to show up inside
   their app as a plugin.

## Already built

Agent blocks are ACP clients, so each agent's questions, approvals and tool
calls show up as cards.

| Platform feature | What Arugula does with it | Where |
| --- | --- | --- |
| ACP (Agent Client Protocol) | Agent blocks are ACP clients. They drive Claude Code (`claude-agent-acp`), Codex (`codex-acp`) and Fountain agents. Gemini CLI and opencode are untested | [DECISIONS.md](../DECISIONS.md), M6b, S7 |
| Claude's `AskUserQuestion` | Shown as a question card through ACP form elicitation, and answerable from a push notification. URL elicitations (an MCP server's sign-in) show as cards too | M6c in [plan-archive.md](plan-archive.md) |
| Codex plan-mode questions | Shown as cards through elicitation. In its default mode Codex hangs: `codex-acp` drops the async question tool | M6c, S13 |
| Claude Code permission prompts and questions in a terminal | Become cards through hooks (`arugula hooks install`), and anyone on the team who may answer can answer | [#46](https://github.com/arugula-salad/arugula/issues/46) (M29), [#229](https://github.com/arugula-salad/arugula/issues/229) |
| MCP, as a server | `arugula mcp` gives any MCP client (Claude Code, Codex, Claude Desktop) tools to run panes, show blocks, draft forge actions and supervise agents | [#5](https://github.com/arugula-salad/arugula/issues/5) (M16), [#349](https://github.com/arugula-salad/arugula/issues/349) |
| Claude Code conversations, from a terminal or the desktop app's Code tab | Indexed as blocks you can read, continue or fork. Claude Desktop chats are out of scope because they live on claude.ai | [#72](https://github.com/arugula-salad/arugula/issues/72) (M33), [#83](https://github.com/arugula-salad/arugula/issues/83) |
| Skills and MCP servers bundled with an agent | A Fountain agent can be worn locally: its skills become a local plugin and its MCP servers are passed by reference, never on argv. Behind labs | [#121](https://github.com/arugula-salad/arugula/issues/121) (M43), [#122](https://github.com/arugula-salad/arugula/issues/122) (M44) |
| Agent inventory | The daemon runs `chant audit --agents` at start to list the agents, skills and servers on a machine | [#255](https://github.com/arugula-salad/arugula/issues/255) |
| Images in prompts | Sent as ACP image content blocks when the agent supports them | [#249](https://github.com/arugula-salad/arugula/issues/249) (M70) |
| Push notifications | Web Push when an agent needs input or a long command ends. This is what Claude Code's Remote Control does for Anthropic's apps | M3, M21 |

In `experimental`, the App builder streams Fountain Conversations and
shows tool approvals as cards. Free-form questions aren't events in Fountain
yet ([PLAN.md](https://github.com/arugula-salad/experimental/blob/main/docs/PLAN.md)).

## Already planned

The AGENTS track is the only planned work that brings in another platform's
protocol. It covers A2A between teammates, not between vendors.

| Plan | What it brings | Status | Where |
| --- | --- | --- | --- |
| AGENTS track: agent catalog and A2A | A recipe is a plain Claude Code subagent file (`.claude/agents/NAME.md`) with an A2A 1.0 card. A team catalog, delegation over control's end-to-end channel, consent cards, and results returned as a reviewed patch | S34 spike is a go. M76 to M80 are open. Outside A2A agents are listed as later | [#403](https://github.com/arugula-salad/arugula/issues/403), [#397](https://github.com/arugula-salad/arugula/issues/397) to [#402](https://github.com/arugula-salad/arugula/issues/402), [#415](https://github.com/arugula-salad/arugula/issues/415) |
| MCP discoverability | A `capabilities` tool, and one Claude Code plugin bundling the MCP server, the hooks and a short skill | Hooks half landed ([#238](https://github.com/arugula-salad/arugula/issues/238)). The plugin and the tool are open | [#229](https://github.com/arugula-salad/arugula/issues/229) |
| Typed MCP and one operation list | Each operation declared once for the API, MCP and the CLI | Design notes, open | [#448](https://github.com/arugula-salad/arugula/issues/448), [#451](https://github.com/arugula-salad/arugula/issues/451) |
| Phone as a hand | The phone's camera, location and Shortcuts as MCP tools for agents. A2A only if the phone acts as an agent itself | Spike closed | [#268](https://github.com/arugula-salad/arugula/issues/268) (S33) |
| Triggers and Goals (`experimental`) | Work that starts from events, not only from a person typing. The grow loop around a Goal | Out of the MVP. "Which triggers come first?" is an open question for Jake and Aaron | [product.md](https://github.com/arugula-salad/experimental/blob/main/docs/product.md), [upstream-questions.md](https://github.com/arugula-salad/experimental/blob/main/docs/upstream-questions.md) |

## Gaps

No doc or issue in either repo covers recurring agent tasks, MCP Apps, Codex's
app-server, or showing up as a plugin inside Claude or ChatGPT. These are the
features most likely to send Joe back to a vendor app.

| What Joe does elsewhere | Vendor feature | Way in | Open or API? |
| --- | --- | --- | --- |
| Runs an agent every morning or on an event | Cowork scheduled tasks (cloud-run as of 2026-10-06), Claude Code Routines (schedule, HTTP or GitHub events), Channels | Build our own: a scheduled or event-started agent block on a machine, with its result as a card and in the pane's thread. Routines can also be started over HTTP | Ours to build. Routines have an HTTP trigger |
| Opens a tool's interactive UI (a chart, a form, a viewer) | MCP Apps: tools return `ui://` HTML, drawn by Claude, ChatGPT, VS Code and Goose | Draw MCP Apps in a block. In an agent block the agent, not Arugula, is the MCP client, so we only see the UI if the ACP adapter passes the tool's `_meta.ui` through. Needs a spike | Open spec, ratified 2026-01-26 |
| Uses his ChatGPT or Claude plugins and connectors | ChatGPT apps are now plugins built on MCP, and one plugin reaches ChatGPT, ChatGPT Work and Codex. Claude plugins bundle skills and connectors | Two directions. Bring his skills and MCP servers into Arugula's agents (M44's wear code already does this for Fountain and M76 recipes). And ship Arugula's MCP server as a plugin in Claude and ChatGPT, so his work there lands in Arugula's cards | Open (MCP, `SKILL.md`) |
| Works in Codex's own apps for plan mode and approvals | Codex app-server: the JSON-RPC protocol behind the Codex CLI, VS Code, web, desktop, Xcode and JetBrains clients | A Codex block on the app-server, where `codex-acp` falls short (the default-mode question hang) | Open protocol, backward compatible |
| Uses Gemini | Gemini CLI speaks ACP. Gemini Enterprise registers outside A2A agents | Test Gemini CLI in an agent block. After the AGENTS track, publish A2A cards outward | Open (ACP, A2A) |
| Runs long jobs with no terminal | Claude Managed Agents: hosted sessions, memory stores, rubric-graded Outcomes, webhooks (beta) | A second runtime beside machines and Fountain, shown as an agent block | API. Billing and token rules need checking |
| Designs, chats with a Project's memory, makes artifacts in the chat | Claude Design, claude.ai Projects and memory, ChatGPT features in its own app | No API. Rebuild the habit (preview blocks, artifacts as MCP Apps) or link out | Vendor app only |

**One thing to check first.** `experimental` decision D5 has people
bring their own token from `claude setup-token`. Running real Claude Code with
it inside a machine is likely fine. Driving the Agent SDK or Managed Agents
with a subscription token may not be allowed. Check before building on either.

## Proposed order

Start with recurring agent tasks: they are the habit most likely to pull Joe
back to Cowork, and nothing covers them yet.

1. **Scheduled and event-started agent blocks.** A spike in Arugula: an agent
   block with a schedule or a trigger (a forge webhook, a failed build), its
   result as a card and in the pane's thread. This would also answer "which
   triggers come first?" for `experimental`.
2. **Finish #229, then ship it as a plugin in Claude and ChatGPT.** The
   `capabilities` tool and the Claude Code plugin are already scoped. A plugin
   in their apps means a Cowork task or a ChatGPT chat can post into Arugula's
   cards.
3. **An MCP Apps spike.** Find out whether `claude-agent-acp` and `codex-acp`
   pass a tool's `ui://` resource through. If they do, draw it in a block. If
   they don't, that's an upstream PR.
4. **The AGENTS track as planned (#403).** Then outside A2A cards, so a Gemini
   Enterprise workspace can hand work to an Arugula agent.
5. **A Codex app-server spike,** if the `codex-acp` hang isn't fixed upstream.
   Test Gemini CLI in an agent block at the same time.
6. **Managed Agents as a runtime,** after the token question is settled.

### Questions for Jake and Alex

- **Jake:** should scheduled runs live in the daemon (a machine that's off
  doesn't run them) or in control (which can wake a machine, but must not read
  it)?
- **Jake:** do we take MCP Apps upstream to the ACP adapters, or draw them only
  for Arugula's own tools?
- **Jake:** stay on `codex-acp`, or add a block on the Codex app-server?
- **Alex:** should chant own scheduled runs, so they get gates and run records
  like releases do?
- **Alex:** can `chant audit --agents` also list a person's skills and plugins,
  so we know what to carry into a machine?

## Sources

Repos, read on 2026-10-07: this repo's [README](../README.md),
[DECISIONS.md](../DECISIONS.md), [plan-archive.md](plan-archive.md),
[labs.md](labs.md) and the issues linked above; and
[experimental](https://github.com/arugula-salad/experimental)'s
[PLAN.md](https://github.com/arugula-salad/experimental/blob/main/docs/PLAN.md),
[product.md](https://github.com/arugula-salad/experimental/blob/main/docs/product.md)
and [upstream-questions.md](https://github.com/arugula-salad/experimental/blob/main/docs/upstream-questions.md).

Vendor and protocol pages (from search results; not all opened):

- [Codex App Server](https://developers.openai.com/codex/app-server) and [InfoQ on it](https://www.infoq.com/news/2026/02/opanai-codex-app-server)
- [ChatGPT and Codex plugins on MCP](https://manufact.com/blog/openai-codex-chatgpt-mcp)
- [Agent Client Protocol](https://agentclientprotocol.com) and [ACP in 2026](https://www.noqta.tn/en/blog/agent-client-protocol-acp-universal-ai-coding-editor-standard-2026)
- [MCP Apps announcement](https://blog.modelcontextprotocol.io/posts/2025-11-21-mcp-apps/) and [MCP Apps spec 2026](https://mcp.directory/blog/mcp-apps-spec-2026-when-should-your-server-render-ui)
- [Cowork on web, desktop and mobile](https://support.claude.com/en/articles/15520349-use-claude-cowork-on-web-desktop-and-mobile) and [Cowork merged into Claude](https://coworkstack.ai/blog/cowork-is-now-claude/)
- [Claude Code routines, Remote Control and Channels](https://rickhigh.substack.com/p/claude-code-advanced-the-four-ways)
- [Managed Agents, May 2026 update](https://mer.vin/2026/05/dreaming-outcomes-and-webhooks-claude-managed-agents-update-may-2026/?amp=1)
- [Register A2A agents in Gemini Enterprise](https://docs.cloud.google.com/gemini/enterprise/docs/register-and-manage-an-a2a-agent)
- [AG-UI](https://www.copilotkit.ai/blog/ag-ui-is-redefining-the-agent-user-interaction-layer)
