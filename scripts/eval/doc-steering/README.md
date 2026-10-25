# Doc steering eval (#1486)

Which tool does interactive Claude Code reach for first when asked for a
write-up: a Doc, an Artifact, or Claude Docs? `claude -p` has no `Artifact`
tool, so the runs are interactive, in panes of throwaway daemons.

    run.sh tools A_BIN B_BIN   # check Artifact is in the tool list
    run.sh eval  A_BIN B_BIN   # 5 prompts (prompts.txt) x 2 runs x 2 conditions

`A_BIN` holds `arugula` and `arugulad` built from `origin/main`, `B_BIN` the
branch's. Each condition runs its own daemon under `mktemp -d` (`docs` on) and
a fresh `claude` per run with `--setting-sources project`, an MCP config for
that daemon only, and `record-tool.sh` as a `PreToolUse` hook, which logs every
tool and denies the three that would publish. B adds the `SessionStart`
`arugula hook`. Needs a logged-in `claude`; nothing is published.
