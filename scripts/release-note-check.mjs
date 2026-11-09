#!/usr/bin/env node
// Is the note on stdin one scripts/release-notes.mjs can read, and is its id new?
// scripts/pr-notes-check runs it on each note a pull request adds.
//
//   git show HEAD:docs/notes/NAME.md | node scripts/release-note-check.mjs NAME.md [new|edited]
//
// An edited note (one already on main) may be in a release already, so only a
// new one's id is checked against the releases.
import { readdirSync, readFileSync } from "node:fs";
import { parseNote } from "./release-notes.mjs";

const name = process.argv[2];
const edited = process.argv[3] === "edited";
try {
  const note = parseNote(name, readFileSync(0, "utf8"));
  // A release refuses a headline id an earlier release used (scripts/whats-new.mjs).
  for (const f of edited ? [] : readdirSync("docs/releases")) {
    if (!/^\d+\.\d+\.\d+\.md$/.test(f)) continue;
    const m = readFileSync(`docs/releases/${f}`, "utf8").match(/^```whats-new\n([\s\S]*?)^```$/m);
    if (m && JSON.parse(m[1]).some((i) => i.id === note.id)) {
      throw new Error(`docs/notes/${name}: ${f} already used the id ${note.id}: rename the file`);
    }
  }
} catch (e) {
  console.error(`notes: ${e.message}`);
  process.exit(1);
}
