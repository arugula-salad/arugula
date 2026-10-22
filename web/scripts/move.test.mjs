// Moving a beta Cloud workspace off Managoat (M189): how the page says
// what a move leaves behind, and the deletion day. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { deletionDay, leftBehind, leftWords } from "../src/platform/cloud/move.ts";

const repo = (over = {}) => ({
  mount: "/home/sprite/app", url: "https://github.com/a/app", branch: "main", changes: [], uncommitted: false,
  unpushed: false, compared_to: "HEAD@{upstream}", truncated: false, error: null, ...over,
});
const change = (path, worktree = "modified") => ({ path, index: "unchanged", worktree });

test("what a repo leaves behind, in words", () => {
  assert.equal(leftWords(repo()), "Nothing left behind.");
  assert.equal(leftWords(repo({ changes: [change("a")], uncommitted: true })), "1 changed or untracked file");
  assert.equal(
    leftWords(repo({ changes: [change("a"), change("b", "untracked")], unpushed: true, compared_to: "origin/HEAD" })),
    "2 changed or untracked files · commits not on origin/HEAD",
  );
  assert.equal(leftWords(repo({ unpushed: null })), "no upstream to compare commits with");
  assert.equal(leftWords(repo({ uncommitted: true, truncated: true })), "uncommitted changes More than this; the download has what was read.");
});

test("only repos with work are left behind", () => {
  const repos = [repo(), repo({ mount: "/x", unpushed: true }), repo({ mount: "/y", changes: [change("n", "untracked")] })];
  assert.deepEqual(leftBehind(repos).map((r) => r.mount), ["/x", "/y"]);
});

test("the deletion day is that day", () => {
  assert.equal(deletionDay("2026-11-05", "en-US"), "Thursday, November 5");
});
