// The nvim key tips' shape (shell/hints/nvim-tips.ts): 40 to 60 of them,
// unique kebab-case ids, every field filled, each line short enough for a
// pane-header chip. That each `help` tag exists in nvim is
// scripts/check-nvim-tips's job (`just nvim-tips-check`). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { NVIM_TIPS } from "../src/shell/hints/nvim-tips.ts";

test("there are 40 to 60 tips", () => {
  assert.ok(NVIM_TIPS.length >= 40 && NVIM_TIPS.length <= 60, `${NVIM_TIPS.length} tips`);
});

test("ids are unique and kebab-case", () => {
  const seen = new Set();
  for (const { id } of NVIM_TIPS) {
    assert.match(id, /^[a-z0-9]+(?:-[a-z0-9]+)*$/, id);
    assert.ok(!seen.has(id), `${id} twice`);
    seen.add(id);
  }
});

test("every field is filled, and the text is one short line", () => {
  for (const tip of NVIM_TIPS) {
    for (const k of ["id", "keys", "text", "help"]) {
      assert.equal(typeof tip[k], "string", `${tip.id}.${k}`);
      assert.ok(tip[k].trim().length > 0, `${tip.id}.${k} is empty`);
      assert.equal(tip[k], tip[k].trim(), `${tip.id}.${k} has spaces around it`);
    }
    assert.ok(tip.text.length <= 80, `${tip.id}: ${tip.text.length} chars`);
    assert.ok(!/[\n!]|\bsimply\b/i.test(tip.text), `${tip.id}: ${tip.text}`);
    assert.ok(!/\s/.test(tip.help), `${tip.id}: a help tag has no spaces`);
    if (tip.since !== undefined) assert.match(tip.since, /^0\.\d+$/, tip.id);
  }
});
