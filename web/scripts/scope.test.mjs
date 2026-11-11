// What Boards, Docs and the Inbox are scoped to (Arugula #881). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { inboxPath, scopable, scopeOf } from "../src/platform/scope.ts";
import { parse, pathOf } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const Q = "prj_00112233445566778899aabbccddeeff";
const known = (id) => id === P;

test("they open in the current project, or all without one", () => {
  for (const path of ["/boards", "/docs", "/inbox"]) {
    assert.equal(scopeOf(parse(path), P, known), P, path);
    assert.equal(scopeOf(parse(path), null, known), undefined, path);
  }
});

test("a current project that isn't the reader's scopes nothing", () => {
  assert.equal(scopeOf(parse("/inbox"), Q, known), undefined);
});

test("?all=1 widens them, and the address says so both ways", () => {
  for (const [path, page] of [["/boards", "all-boards"], ["/docs", "docs"], ["/inbox", "inbox"]]) {
    const r = parse(path, "?all=1");
    assert.deepEqual(r, { page, all: true });
    assert.equal(scopeOf(r, P, known), undefined);
    assert.equal(pathOf(r), `${path}?all=1`);
    assert.deepEqual(parse(path), { page });
    assert.equal(pathOf({ page }), path);
  }
  assert.deepEqual(parse("/inbox", "?all=0"), { page: "inbox" });
});

test("only the pages outside a project are scopable", () => {
  assert.equal(scopable(parse("/inbox")), true);
  assert.equal(scopable(parse("/boards")), true);
  assert.equal(scopable(parse("/docs")), true);
  assert.equal(scopable(parse(`/p/${P}/docs`)), false);
  assert.equal(scopable(parse("/settings")), false);
});

test("the inbox asks the platform for the project, or for everything", () => {
  assert.equal(inboxPath(P), `/api/inbox?project=${P}`);
  assert.equal(inboxPath(undefined), "/api/inbox");
});
