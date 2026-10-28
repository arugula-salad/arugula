// Triage's words and the Add to board… menu's model (#292).
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { issuesPlace, itemHref, sourceLabel, targets, without } from "../src/platform/pages/triage-text.ts";
import { parse, pathOf } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const I = "iss_0123456789abcdef0123456789abcdef";

test("the bar's Issues place becomes Triage only once boards are on", () => {
  assert.equal(issuesPlace(null), "issues");
  assert.equal(issuesPlace("off"), "issues");
  assert.equal(issuesPlace([]), "triage");
  assert.equal(issuesPlace([{ id: "brd_1" }]), "triage");
});

test("a source label is OWNER/NAME#N, or #N for a native issue", () => {
  assert.equal(sourceLabel({ repo: "acme/app", number: 7 }), "acme/app#7");
  assert.equal(sourceLabel({ repo: null, number: 12 }), "#12");
  assert.equal(sourceLabel({ number: null }), "");
});

test("a native issue links to its page, a GitHub one to github.com", () => {
  const path = (id) => `/p/x/issues/${id}`;
  assert.equal(itemHref({ source: "native", pointer: { kind: "issue", id: I }, number: 3 }, path), `/p/x/issues/${I}`);
  assert.equal(itemHref({ source: "github", pointer: { kind: "forge_item", id: "fi_1" }, repo: "acme/app", number: 7 }, path), "https://github.com/acme/app/issues/7");
  assert.equal(itemHref({ source: "github", pointer: { kind: "forge_item", id: "fi_1" }, repo: "not a repo", number: 7 }, path), undefined);
});

test("the menu is each board's zones but Proofs, and a board with none is out", () => {
  const zone = (id, kind) => ({ id, title: id.toUpperCase(), kind, rows: [] });
  const views = [
    { board: { id: "brd_a", title: "Ship" }, zones: [zone("z1"), zone("z2", null), zone("zp", "proofs")] },
    { board: { id: "brd_b", title: "Empty" }, zones: [zone("zq", "proofs")] },
  ];
  assert.deepEqual(targets(views), [{ board: "brd_a", title: "Ship", zones: [{ id: "z1", title: "Z1" }, { id: "z2", title: "Z2" }] }]);
});

test("an item put on a board leaves the list", () => {
  const a = { pointer: { kind: "issue", id: "iss_a" } };
  const b = { pointer: { kind: "forge_item", id: "fi_b" } };
  assert.deepEqual(without([a, b], a.pointer), [b]);
});

test("Triage is /p/:project/triage", () => {
  assert.deepEqual(parse(`/p/${P}/triage`), { page: "triage", project: P });
  assert.equal(pathOf({ page: "triage", project: P }), `/p/${P}/triage`);
  assert.equal(parse(`/p/${P}/triage/extra`).page, "missing");
});
