// Claude Code's plugin (plugins/claude-code), offered where the hooks are
// (Discovery 10): Getting started's Agents step and the unhooked chip. One
// plugin brings Arugula's MCP tools and its hooks, instead of `claude mcp
// add` and `arugula hooks install`; these are the two lines to type in
// Claude Code, as the README says them. Getting started also offers its live
// session signals (#447), an option that's off by default: the line that
// turns it on from a shell, and where it is in Claude Code.

import { CopyText } from "../../shared/ui/copy";

/** The two `/plugin` lines, in the order Claude Code takes them. */
export const PLUGIN_LINES = ["/plugin marketplace add arugula-salad/arugula", "/plugin install arugula@arugula"];

/** Turns on the plugin's live session signals from a shell, as `arugula
 * hooks install` says it (crates/cli/src/hooks.rs `live_offer`). */
export const LIVE_LINE = `echo '{"live":"true"}' | claude plugin configure arugula@arugula --values-stdin`;

/** The plugin as the alternative: a line saying what it is, then each
 * command to copy; with `live`, its opt-in live session signals after. */
export function ClaudePlugin({ lead = "Or, instead, Claude Code's plugin brings both. In Claude Code:", live = false }: { lead?: string; live?: boolean }) {
  return (
    <div class="claude-plugin" data-claude-plugin>
      <p class="start-dim">{lead}</p>
      {PLUGIN_LINES.map((l) => (
        <CopyText key={l} text={l} data-plugin-command />
      ))}
      {live && (
        <>
          <p class="start-dim" data-plugin-live>
            Optional, off by default: live session signals, the context window's fill, the model and compactions, on the pane as they happen (in a terminal;
            the desktop app, VS Code and hosted sessions aren't checked yet). Turn them on when you install, in <code>/config</code>, or:
          </p>
          <CopyText text={LIVE_LINE} data-plugin-live-command />
        </>
      )}
    </div>
  );
}
