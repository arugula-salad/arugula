// The nvim key tips' shape (shell/hints/nvim-tips.ts): 40 to 60 of them,
// unique kebab-case ids, every field filled, each line short enough for a
// pane-header chip. That each `help` tag exists in nvim is
// scripts/check-nvim-tips's job (`just nvim-tips-check`). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { NVIM_TIPS, nextTip, nvimAtLeast, tipDocs } from "../src/shell/hints/nvim-tips.ts";

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
    for (const k of ["id", "keys", "text", "help", "page"]) {
      assert.equal(typeof tip[k], "string", `${tip.id}.${k}`);
      assert.ok(tip[k].trim().length > 0, `${tip.id}.${k} is empty`);
      assert.equal(tip[k], tip[k].trim(), `${tip.id}.${k} has spaces around it`);
    }
    assert.ok(tip.text.length <= 80, `${tip.id}: ${tip.text.length} chars`);
    assert.ok(!/[\n!]|\bsimply\b/i.test(tip.text), `${tip.id}: ${tip.text}`);
    assert.ok(!/\s/.test(tip.help), `${tip.id}: a help tag has no spaces`);
    if (tip.since !== undefined) assert.match(tip.since, /^0\.\d+$/, tip.id);
    assert.match(tip.page, /^[a-z0-9_-]+\/#\S+$/, `${tip.id}: page ${tip.page}`);
  }
});

test("a version is compared by major and minor; an unknown one takes every tip", () => {
  assert.equal(nvimAtLeast("0.11.6", "0.11"), true);
  assert.equal(nvimAtLeast("0.12.0", "0.11"), true);
  assert.equal(nvimAtLeast("1.0.0", "0.11"), true);
  assert.equal(nvimAtLeast("0.10.4", "0.11"), false);
  assert.equal(nvimAtLeast(undefined, "0.11"), true);
  assert.equal(nvimAtLeast("nonsense", "0.11"), true);
});

test("each tip once before any repeats, skipping those the nvim is too old for", () => {
  const tips = [
    { id: "a", keys: "a", text: "A", help: "a", page: "x/#a" },
    { id: "b", keys: "b", text: "B", help: "b", page: "x/#b", since: "0.11" },
    { id: "c", keys: "c", text: "C", help: "c", page: "x/#c" },
  ];
  const walk = (version, n) => {
    const out = [];
    let from = 0;
    for (let i = 0; i < n; i++) {
      const at = nextTip(from, version, tips);
      out.push(tips[at].id);
      from = at + 1;
    }
    return out.join("");
  };
  assert.equal(walk("0.11.6", 6), "abcabc");
  assert.equal(walk("0.10.4", 4), "acac");
  assert.equal(nextTip(-3, null, tips), 0);
  assert.equal(nextTip(7, null, tips), 1);
  // The real list from its start: every tip, then the first again.
  const seen = new Set();
  let from = 0;
  for (let i = 0; i < NVIM_TIPS.length; i++) {
    const at = nextTip(from, "0.11.6");
    seen.add(NVIM_TIPS[at].id);
    from = at + 1;
  }
  assert.equal(seen.size, NVIM_TIPS.length);
  assert.equal(nextTip(from, "0.11.6"), 0);
});

test("a tip's online docs are its page, else nvim's lookup by tag", () => {
  assert.equal(tipDocs({ help: "gv", page: "visual/#gv" }), "https://neovim.io/doc/user/visual/#gv");
  assert.equal(tipDocs({ help: "]<Space>" }), "https://neovim.io/doc/user/helptag/?tag=%5D%3CSpace%3E");
});
