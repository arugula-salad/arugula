// Issues on the platform page (M201): what a timeline event says, who did
// it (a person, or their agent, M197), a list's query, and the issue
// routes' paths. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ago, eventLine, githubNewIssue, labelsOf, queryOf, reasonLabel, stateLabel, whoLabel } from "../src/platform/issues/text.ts";

import { parse, pathOf } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const I = "iss_0123456789abcdef0123456789abcdef";
const nameOf = (a) => ({ a1: "Jake", a2: "Sam" })[a];
const ev = (kind, detail = {}, over = {}) => ({ id: 1, kind, actor: "a1", agent: null, at: 0, detail, ...over });

test("who did it: a person, someone gone, or their agent", () => {
  assert.equal(whoLabel("a1", null, nameOf), "Jake");
  assert.equal(whoLabel("a9", null, nameOf), "someone who left");
  assert.equal(whoLabel("", null, nameOf), "a deleted account");
  assert.equal(whoLabel("a1", { machine: "d1", machine_name: "geek" }, nameOf), "Jake's agent on geek");
});

test("each event says what it did", () => {
  assert.equal(eventLine(ev("opened"), nameOf), null);
  assert.equal(eventLine(ev("edited", { title: "Old" }), nameOf), "changed the title from “Old”");
  assert.equal(eventLine(ev("edited", { body: true }), nameOf), "edited the description");
  assert.equal(eventLine(ev("edited", { title: "Old", body: true }), nameOf), "changed the title from “Old” and edited the description");
  assert.equal(eventLine(ev("closed", { reason: "not_planned" }), nameOf), "closed this as not planned");
  assert.equal(eventLine(ev("closed", {}), nameOf), "closed this as completed");
  assert.equal(eventLine(ev("reopened"), nameOf), "reopened this");
  assert.equal(eventLine(ev("labelled", { label: "bug" }), nameOf), "added the label bug");
  assert.equal(eventLine(ev("unlabelled", { label: "bug" }), nameOf), "removed the label bug");
  assert.equal(eventLine(ev("assigned", { assignee: "a2" }), nameOf), "assigned Sam");
  assert.equal(eventLine(ev("assigned", { assignee: "a1" }), nameOf), "self-assigned this");
  // An agent assigning its owner isn't the owner assigning themselves.
  assert.equal(eventLine(ev("assigned", { assignee: "a1" }, { agent: { machine: "d1", machine_name: "geek" } }), nameOf), "assigned Jake");
  assert.equal(eventLine(ev("unassigned", { assignee: "a9" }), nameOf), "unassigned someone who left");
  assert.equal(eventLine(ev("exported", { repo: "acme/app", number: 7 }), nameOf), "sent this to GitHub as acme/app#7");
});

test("states and reasons", () => {
  assert.equal(stateLabel({ state: "open", close_reason: null }), "Open");
  assert.equal(stateLabel({ state: "closed", close_reason: "completed" }), "Closed");
  assert.equal(stateLabel({ state: "closed", close_reason: "not_planned" }), "Closed: not planned");
  assert.equal(reasonLabel("duplicate"), "a duplicate");
  assert.equal(reasonLabel(null), "completed");
});

test("ago, and labels typed in a box", () => {
  const now = 1_800_000_000_000;
  assert.equal(ago(now - 10_000, now), "just now");
  assert.equal(ago(now - 60_000, now), "1 minute ago");
  assert.equal(ago(now - 3 * 3_600_000, now), "3 hours ago");
  assert.equal(ago(now - 2 * 86_400_000, now), "2 days ago");
  assert.match(ago(now - 90 * 86_400_000, now), /^on /);
  assert.deepEqual(labelsOf(" bug, web,,bug ,"), ["bug", "web"]);
});

test("a list's query: open by default, the rest only when given", () => {
  assert.equal(queryOf({}), "state=open");
  assert.equal(queryOf({ state: "closed", label: "bug", assignee: "a2", q: "  dry-run " }, 12, 5), "state=closed&labels=bug&assignee=a2&q=dry-run&before=12&limit=5");
  assert.equal(queryOf({ q: "  " }), "state=open");
});

test("the issue routes, by id with a slug", () => {
  const names = (id) => ({ [P]: "Acme", [I]: "Search misses words" })[id];
  assert.deepEqual(parse(`/p/${P}-acme/issues`), { page: "issues", project: P });
  assert.deepEqual(parse(`/p/${P}/issues`, "?state=closed&q=dry&label=bug&assignee=a2"), { page: "issues", project: P, state: "closed", q: "dry", label: "bug", assignee: "a2" });
  assert.deepEqual(parse(`/p/${P}/issues`, "?state=open"), { page: "issues", project: P }, "open is the default");
  assert.deepEqual(parse(`/p/${P}/issues/new`), { page: "new-issue", project: P });
  assert.deepEqual(parse(`/p/${P}/issues/${I}-an-old-title`, "?msg=3"), { page: "issue", project: P, issue: I, msg: 3 });
  assert.deepEqual(parse(`/issues/${I}-x`), { page: "find", what: "issue", issue: I });
  // A push for a mention in its thread (M202) opens it at the message.
  assert.deepEqual(parse(`/issues/${I}`, "?msg=4"), { page: "find", what: "issue", issue: I, msg: 4 });
  assert.equal(pathOf({ page: "find", what: "issue", issue: I, msg: 4 }), `/issues/${I}?msg=4`);
  for (const p of [`/p/${P}/issues/12`, `/p/${P}/issues/ch_0123456789abcdef`, `/p/${P}/issues/${I}/x`, "/issues/12", "/issues"]) assert.equal(parse(p).page, "missing", p);
  assert.equal(pathOf({ page: "issue", project: P, issue: I }, names), `/p/${P}-acme/issues/${I}-search-misses-words`);
  for (const r of [
    { page: "issues", project: P, state: "closed", q: "has words", label: "good first", assignee: "a2" },
    { page: "new-issue", project: P },
    { page: "issue", project: P, issue: I, msg: 4 },
    { page: "find", what: "issue", issue: I },
  ]) {
    const [p, q] = pathOf(r, names).split("?");
    assert.deepEqual(parse(p, q ? `?${q}` : ""), r, p);
  }
});

test("GitHub's new issue page, each part of the repo escaped", () => {
  assert.equal(githubNewIssue("acme/app"), "https://github.com/acme/app/issues/new");
  assert.equal(githubNewIssue("ac me/a?p#x"), "https://github.com/ac%20me/a%3Fp%23x/issues/new");
});
