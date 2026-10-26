---
name: arugula
description: How to work in Arugula, where your terminal is a pane people watch and take over from the web, the phone and the desktop app. Use when you run inside an Arugula pane (ARUGULA_PANE is set) or have Arugula's MCP tools, before starting builds or servers, following long output, showing the user a diff, a port, a PR or an issue, or writing to a forge.
user-invocable: false
---

# Working in Arugula

Arugula's MCP server explains its tools in its own instructions; this is the order to use them in.

1. **Call `capabilities` first.** It says which blocks you can show here, which forges the user is signed in to, whether this machine joined control, and whether Claude Code's hooks are live in your pane. Plan around what it says, not around what you expect.
2. **Run long things in panes.** `run` starts a build, test or dev server in a pane the user can watch (`wait: true` to wait for it). Follow it with `wait` (until it ends, matches a pattern, goes idle or asks for input), never with `sleep` and repeated `read_output`. `wait` and `read_output` answer "still running" with an offset: call again with it.
3. **Show, don't describe.** Put the thing beside the pane with `show`: `changes` for a diff you made, `port` for a dev server, `file` at a line, `pr` or `issue` for one on the forge, `conversation` for a Claude Code conversation to continue or fork. A report, plan or write-up longer than a screen goes in a Doc (the `doc` tool), if `capabilities` lists one, not in an Artifact or another tool's page and never as a long reply: the user watches it fill in beside your pane. When they want to show it to someone, ask with `doc share`: the user decides, and you get the link back.
4. **File issues in Arugula; draft, don't act, on forges.** A new issue (a bug, a follow-up, a ticket for later) goes in Arugula's own issues with `issue_write` (kind `create`), not `gh issue create` or a GitHub draft, unless the user asks for GitHub; `forge_read` (kind `native_issues`) lists them. On GitHub, a comment, review, merge or new issue goes through `draft`: it waits as a card the user edits and sends, or drops. Nothing reaches the forge in your name. `read_forge` shows what became of it.
5. **Hooks.** When `capabilities` says hooks are live, your questions (AskUserQuestion) and permission prompts are cards anyone allowed can answer from the phone, and follow-ups reach you through `arugula inbox`. When they aren't, they stay in the terminal: say so to your person rather than installing anything yourself (`arugula hooks status` says what's there).
6. **Run each `arugula ...` shell command on its own**, not in a pipeline or an `&&` chain, so permission rules match it.

If a tool fails because the daemon isn't running or `arugula` isn't on PATH, say so plainly and carry on without Arugula; don't start a daemon yourself.
