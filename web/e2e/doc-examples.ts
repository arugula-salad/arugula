// The Doc grammar's examples (#964): crates/daemon/tests/fixtures/docs/.
// The daemon's validator (embeds.rs) and the web client's (ui/chart.tsx,
// ui/embeds.ts) both run over these files, and expect.json says what each
// must come to, so the two can't drift.

import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const DIR = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/docs/", import.meta.url));

/** A file's text, by its path under the fixtures (`bad/bad-json.md`). */
export const example = (name: string): string => readFileSync(DIR + name, "utf8");

/** `good`: each good example's text with its fences as lines; `bad`: each
 * bad one's file path and what its refusal says. */
export const expected = JSON.parse(example("expect.json")) as { good: Record<string, string>; bad: Record<string, string> };

/** The `.md` files in a fixtures directory, as paths under it, sorted. */
export const examples = (sub = ""): string[] =>
  readdirSync(DIR + sub)
    .filter((f) => f.endsWith(".md"))
    .map((f) => (sub ? `${sub}/${f}` : f))
    .sort();
