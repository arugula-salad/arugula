// Claude Code's plugin (plugins/claude-code), offered where the hooks are
// (Discovery 10): Getting started's Agents step and the unhooked chip. One
// plugin brings Arugula's MCP tools and its hooks, instead of `claude mcp
// add` and `arugula hooks install`; these are the two lines to type in
// Claude Code, as the README says them.

import { CopyText } from "../../shared/ui/copy";

/** The two `/plugin` lines, in the order Claude Code takes them. */
export const PLUGIN_LINES = ["/plugin marketplace add arugula-salad/arugula", "/plugin install arugula@arugula"];

/** The plugin as the alternative: a line saying what it is, then each
 * command to copy. */
export function ClaudePlugin({ lead = "Or, instead, Claude Code's plugin brings both. In Claude Code:" }: { lead?: string }) {
  return (
    <div class="claude-plugin" data-claude-plugin>
      <p class="start-dim">{lead}</p>
      {PLUGIN_LINES.map((l) => (
        <CopyText key={l} text={l} data-plugin-command />
      ))}
    </div>
  );
}
