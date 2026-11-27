// Claude Code's screen, as the agent panel folds it (Lift 4, Arugula #1480):
// a terminal's last lines cut into prose and tool calls, a call one line
// with its results under it, runs of reads or edits one line, and its
// chrome (banner, the box you write in, spinners, timings) left out. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { claudeParts, isClaudeScreen } from "../src/shared/transcript.ts";

const fixture = (name) => readFileSync(new URL(`../../crates/vt/fixtures/screens/${name}`, import.meta.url), "utf8").replace(/^title: .*\n/, "");

test("the vt fixture's screen: what you wrote, the call folded with its result, what it said; no banner, timings or footer", () => {
  const parts = claudeParts(fixture("claude_transcript.txt"));
  assert.deepEqual(parts, [
    { kind: "prose", text: "**You** Run this shell command: sleep 4 && touch made-by-claude.txt" },
    { kind: "tool", line: "Bash sleep 4 && touch made-by-claude.txt", body: "(No output)" },
    { kind: "prose", text: "Done. The file made-by-claude.txt has been created in the working directory\nafter a 4-second delay." },
  ]);
});

test("a run of reads is one line of names, a run of edits says how many; a call already folded keeps its words", () => {
  const screen = [
    "⏺ Read(web/src/shell/Crumb.tsx)",
    "  ⎿  Read 120 lines",
    "⏺ Read(web/e2e/window.spec.ts)",
    "  ⎿  Read 80 lines",
    "",
    "⏺ Update(web/src/shell/Crumb.tsx)",
    "  ⎿  Updated web/src/shell/Crumb.tsx with 42 additions and 11 removals",
    "⏺ Update(web/src/shell/rail.tsx)",
    "  ⎿  Updated web/src/shell/rail.tsx with 2 additions",
    "⏺ Write(web/e2e/crumb.spec.ts)",
    "  ⎿  Wrote 30 lines to web/e2e/crumb.spec.ts",
    "",
    "⏺ Searched for 3 patterns, read 2 files (ctrl+o to expand)",
    "",
    "⏺ The caret now sits beside the crumb.",
    "",
    "✻ Crunched for 7s",
    "",
    "──────────────────────────────",
    "❯ ",
    "──────────────────────────────",
    "  ? for shortcuts",
  ].join("\n");
  assert.ok(isClaudeScreen(screen));
  const parts = claudeParts(screen);
  assert.deepEqual(
    parts.map((p) => (p.kind === "tool" ? `▸ ${p.line}` : p.text)),
    ["▸ Read Crumb.tsx, window.spec.ts", "▸ Edited 3 files · Crumb.tsx, rail.tsx, crumb.spec.ts", "▸ Searched for 3 patterns, read 2 files", "The caret now sits beside the crumb."],
  );
  assert.match(parts[1].body, /Update\(web\/src\/shell\/rail.tsx\)\n {2}Updated web\/src\/shell\/rail.tsx with 2 additions/);
});

test("`said` leaves the tool calls out; a screen with no ⏺ isn't Claude Code's", () => {
  assert.deepEqual(
    claudeParts("⏺ Bash(ls)\n  ⎿  a b\n⏺ Two files.", "said").map((p) => p.kind),
    ["prose"],
  );
  assert.equal(isClaudeScreen("$ ls\na b\n$ "), false);
});

test("what it said keeps its indented lists and tables", () => {
  const parts = claudeParts("⏺ Two things:\n  * one\n  │ a │ b │\n  - two");
  assert.deepEqual(parts, [{ kind: "prose", text: "Two things:\n* one\n│ a │ b │\n- two" }]);
});
