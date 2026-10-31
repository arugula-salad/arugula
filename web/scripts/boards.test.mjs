// A board's words and paths (boards, step 2): a row's glyph, a
// zone's progress, a pointer's label and the id a person types for one, and
// the board routes' paths. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { holdsRow, stalledFor, settledFacts, LANE_GLYPH, cardTone, checkTone, checkWord, groupMove, liveTone, prsOf, answerReady, askWaitingLine, assigneesLine, chipOf, counted, debounced, earlierLine, epicLine, firstLine, groupLine, isCard, laneCount, liveWord, marksOf, readyCount, relationLine, UNSEEN_BLOCKER, needsYou, needsYouLine, parsePointer, pointerHref, pointerLabel, pointerToSend, proofMark, rowGlyph, sections, stepActive, workLabel, workChip, onIssueChip, boardGroup, clusterOrder, NO_BOARD } from "../src/shared/boards/text.ts";
import { parse, pathOf } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const B = "brd_0123456789abcdef0123456789abcdef";

test("a lane is a glyph, a row asking for input is a ?, and a lane counts its proven rows", () => {
  assert.deepEqual(Object.values(LANE_GLYPH), ["○", "◐", "◑", "●"]);
  assert.equal(rowGlyph({ lane: "review", status: "none" }), "◑");
  assert.equal(rowGlyph({ lane: "doing", status: "needs_input" }), "?");
  const rows = [{ proof: { by: "ci" } }, {}, { proof: { by: "evidence" } }];
  assert.deepEqual(laneCount({ rows }), { total: 3, proven: 2 });
  assert.deepEqual(laneCount({ rows: [] }), { total: 0, proven: 0 });
});

test("an epic's progress is in words, and a text's first line is what a list shows", () => {
  assert.deepEqual(epicLine({ done: 3, total: 5, proven: 2 }), { words: "3 of 5 done, 2 proven", pct: 60 });
  assert.deepEqual(epicLine({ done: 0, total: 2, proven: 0 }), { words: "0 of 2 done", pct: 0 });
  assert.deepEqual(epicLine({ done: 0, total: 0, proven: 0 }), { words: "No work yet", pct: 0 });
  assert.equal(firstLine("\n  Ship it  \nand more"), "Ship it");
  assert.equal(firstLine(""), "");
});

test("a pointer is labelled by its item when the platform read it", () => {
  const live = { state: "merged", kind: "pr", repo: "acme/app", number: 7, title: "Fix" };
  assert.equal(pointerLabel({ kind: "forge_item", id: "fi_abc123" }, live), "acme/app#7");
  assert.equal(pointerLabel({ kind: "forge_item", id: "fi_abc123" }), "PR or issue abc123");
  assert.equal(pointerLabel({ kind: "issue", id: "iss_abc123" }, { state: "open", kind: "native", number: 3, title: "x" }), "#3");
  const plane = { state: "open", kind: "issue", repo: "Arugula", number: 12, title: "x", provider: "plane", identifier: "ARUGU", state_name: "In Progress", url: "https://app.plane.so/acme/browse/ARUGU-12/" };
  assert.equal(pointerLabel({ kind: "forge_item", id: "fi_abc123" }, plane), "ARUGU-12");
  assert.equal(liveWord(plane), "In Progress");
  assert.equal(liveWord({ state: "open" }), "open");
  assert.equal(workLabel({ source: "plane", repo: "Arugula", identifier: "ARUGU", number: 12, title: "x" }), "ARUGU-12");
  assert.equal(pointerLabel({ kind: "block", machine: "m1", pane: "p2" }), "agent");
  assert.equal(pointerLabel({ kind: "none" }), "");
  const here = (path) => `https://app.example${path}`;
  assert.equal(pointerHref({ pointer: { kind: "forge_item", id: "fi_abc123" }, live }, here), "https://github.com/acme/app/pull/7");
  assert.equal(pointerHref({ pointer: { kind: "issue", id: "iss_abc123" } }, here), "https://app.example/issues/iss_abc123");
  assert.equal(pointerHref({ pointer: { kind: "block", machine: "m", pane: "p" } }, here), undefined);
  assert.equal(pointerHref({ pointer: { kind: "forge_item", id: "fi_abc123" }, live: plane }, here), "https://app.plane.so/acme/browse/ARUGU-12/");
});

test("an issue or a PR reads as a card, a run does not", () => {
  const live = (kind) => ({ state: "open", kind, number: 1, title: "x" });
  assert.deepEqual(["pr", "issue", "native", "run"].map((k) => isCard(live(k))), [true, true, true, false]);
  assert.equal(isCard(undefined), false);
  assert.equal(isCard(null), false);
});

test("a card's assignees are comma-separated, and shortened past three", () => {
  assert.equal(assigneesLine(undefined), "");
  assert.equal(assigneesLine([]), "");
  assert.equal(assigneesLine(["ana"]), "ana");
  assert.equal(assigneesLine(["ana", "bo", "cy"]), "ana, bo, cy");
  assert.equal(assigneesLine(["ana", "bo", "cy", "di"]), "ana, bo +2");
  assert.equal(assigneesLine(["a", "b", "c", "d", "e"]), "a, b +3");
});

test("a typed pointer is an id of a kind a row may have as its item", () => {
  assert.deepEqual(parsePointer(""), { kind: "none" });
  assert.deepEqual(parsePointer(" iss_0abc "), { kind: "issue", id: "iss_0abc" });
  assert.deepEqual(parsePointer("fi_0abc"), { kind: "forge_item", id: "fi_0abc" });
  assert.equal(parsePointer("hw_0abc"), null, "a workspace is not a row's item");
  assert.equal(parsePointer("run_0abc"), null, "a run is attached, not a row's item");
  assert.deepEqual(parsePointer("https://example.com/x"), { kind: "link", url: "https://example.com/x" });
  assert.equal(parsePointer("http://example.com/x"), null);
  assert.equal(parsePointer("https://example.com/a b"), null);
  assert.equal(parsePointer("brd_0abc"), null);
});

test("a Done row says how it is proven, or that it isn't yet; others say nothing", () => {
  const issue = { kind: "issue", id: "iss_a" };
  assert.deepEqual(proofMark({ lane: "done", pointer: issue, proof: { by: "ci" } }), { proven: true, words: "proven by CI" });
  assert.deepEqual(proofMark({ lane: "done", pointer: issue, proof: { by: "evidence" } }), { proven: true, words: "proven" });
  assert.deepEqual(proofMark({ lane: "done", pointer: issue }), { proven: false, words: "not proven yet" });
  assert.equal(proofMark({ lane: "review", pointer: issue }), null);
  assert.equal(proofMark({ lane: "done", pointer: { kind: "none" } }), null, "a note has nothing to prove");
  assert.equal(counted({ pointer: { kind: "forge_item", id: "fi_a" } }), true);
  assert.equal(counted({ pointer: { kind: "link", url: "https://x.example/1" } }), false);
});

test("an attachment's chip is its label and its state, and says nothing of what the reader can't see", () => {
  const link = { role: "proof", pointer: { kind: "link", url: "https://ci.example.com/1" } };
  assert.deepEqual(chipOf(link), { label: "ci.example.com/1", state: null });
  assert.equal(pointerHref(link, () => undefined), "https://ci.example.com/1");
  const live = { state: "merged", kind: "pr", repo: "acme/app", number: 7, title: "Fix" };
  assert.deepEqual(chipOf({ role: "work", pointer: { kind: "forge_item", id: "fi_abc123" }, live }), { label: "acme/app#7", state: "merged" });
  assert.deepEqual(chipOf({ role: "work", pointer: { kind: "forge_item", id: "fi_abc123" } }), { label: "PR or issue abc123", state: null });
  assert.deepEqual(chipOf({ role: "work", pointer: { kind: "block", machine: "m", pane: "7" }, block: { state: "working" } }), { label: "agent", state: "working" });
});

test("a block's chip is its title, and work on an issue says its board and links to the row (#372)", () => {
  const block = { role: "work", pointer: { kind: "block", machine: "m", pane: "7" } };
  assert.deepEqual(chipOf({ ...block, block: { state: "idle", title: "Lexer lane" } }), { label: "Lexer lane", state: "idle" });
  assert.deepEqual(chipOf({ ...block, block: { state: "idle" } }), { label: "agent", state: "idle" });
  // The pane number is never the name.
  assert.deepEqual(chipOf({ ...block, block: { state: "idle", title: "  " } }), { label: "agent", state: "idle" });
  const on = { project: P, board: B, title: "Launch", row: "row_0a1b" };
  assert.deepEqual(workChip("#12", on), { label: "#12 · Launch", path: `/p/${P}/boards/${B}#row=row_0a1b` });
  assert.deepEqual(workChip("#12", null), { label: "#12", path: null });
  assert.deepEqual(workChip("ARUGU-7"), { label: "ARUGU-7", path: null });
});

test("the board routes read and write the same paths", () => {
  assert.deepEqual(parse(`/p/${P}/boards`), { page: "boards", project: P });
  assert.deepEqual(parse(`/p/${P}/boards/${B}-launch`), { page: "board", project: P, board: B });
  assert.equal(parse(`/p/${P}/boards/iss_0123`).page, "missing");
  const names = (id) => ({ [P]: "Arugula", [B]: "Launch" })[id];
  assert.equal(pathOf({ page: "board", project: P, board: B }, names), `/p/${P}-arugula/boards/${B}-launch`);
  assert.equal(pathOf({ page: "boards", project: P }), `/p/${P}/boards`);
});

test("a result is labelled by its source", () => {
  assert.equal(workLabel({ source: "github", repo: "acme/app", number: 7, title: "Fix" }), "acme/app#7");
  assert.equal(workLabel({ source: "native", repo: null, number: 3, title: "Fix" }), "#3");
  assert.equal(workLabel({ source: "run", repo: null, number: null, title: "manual run" }), "manual run");
});

test("the arrow keys step through the results and wrap", () => {
  assert.equal(stepActive(-1, 3, true), 0);
  assert.equal(stepActive(2, 3, true), 0);
  assert.equal(stepActive(-1, 3, false), 2);
  assert.equal(stepActive(0, 3, false), 2);
  assert.equal(stepActive(1, 3, false), 0);
  assert.equal(stepActive(0, 0, true), -1);
});

test("a search waits for the last keystroke", async () => {
  const seen = [];
  const f = debounced((q) => seen.push(q), 20);
  f("a");
  f("ab");
  await new Promise((r) => setTimeout(r, 60));
  assert.deepEqual(seen, ["ab"]);
  f("c");
  f.cancel();
  await new Promise((r) => setTimeout(r, 60));
  assert.deepEqual(seen, ["ab"]);
});

test("a row sends a pointer that was picked or typed, never one it only kept", () => {
  const picked = { kind: "issue", id: "iss_0abc" };
  // Picked, then the note edited: the picked pointer still goes.
  assert.deepEqual(pointerToSend("#3", "", picked), { pointer: picked });
  assert.deepEqual(pointerToSend("#3", "https://ci.example.com/1", picked), { pointer: picked });
  // A note edited alone: no pointer, so a link or a block's stays.
  assert.deepEqual(pointerToSend("https://ci.example.com/1", "https://ci.example.com/1", null), {});
  assert.deepEqual(pointerToSend("", "", null), {});
  // A typed id or link goes as before; text that is neither is refused.
  assert.deepEqual(pointerToSend("iss_0abc", "", null), { pointer: picked });
  assert.deepEqual(pointerToSend("https://example.com/x", "", null), { pointer: { kind: "link", url: "https://example.com/x" } });
  assert.equal(pointerToSend("some words", "", null), null);
});

test("a board with groups is a section for each, its lanes under it, and the rows with none last", () => {
  const row = (id, group) => ({ id, group });
  const lanes = [
    { id: "lane_next", title: "Next", rows: [row("a", "zon_1"), row("b", "zon_2"), row("c")] },
    { id: "lane_doing", title: "Doing", rows: [] },
    { id: "lane_review", title: "Review", rows: [] },
    { id: "lane_done", title: "Done", rows: [row("d", "zon_1")] },
  ];
  const groups = [
    { id: "zon_1", title: "One", progress: { done: 1, total: 2, proven: 0 } },
    { id: "zon_2", title: "Two", progress: { done: 0, total: 0, proven: 0 } },
  ];
  const got = sections({ zones: lanes, groups });
  assert.deepEqual(got.map((s) => s.group?.title ?? null), ["One", "Two", null]);
  assert.deepEqual(got.map((s) => s.lanes.length), [4, 4, 4], "four lanes in each");
  assert.deepEqual(got.map((s) => s.lanes.flatMap((z) => z.rows.map((r) => r.id))), [["a", "d"], ["b"], ["c"]]);
  assert.deepEqual(got[0].lanes.map((z) => z.id), lanes.map((z) => z.id));
  // Nothing ungrouped: no last section. A row in a group that's gone is ungrouped.
  assert.equal(sections({ zones: lanes.map((z) => ({ ...z, rows: z.rows.filter((r) => r.group) })), groups }).length, 2);
  assert.equal(sections({ zones: [{ id: "lane_next", title: "Next", rows: [row("x", "zon_9")] }], groups }).at(-1).lanes[0].rows[0].id, "x");
  // No groups (or none from an old platform): the lanes as they came.
  assert.deepEqual(sections({ zones: lanes, groups: [] }), [{ group: null, lanes }]);
  assert.deepEqual(sections({ zones: lanes }), [{ group: null, lanes }]);
  assert.equal(groupLine({ done: 1, total: 2 }), "1 of 2 done");
  assert.equal(groupLine({ done: 0, total: 0 }), "no work yet");
});

test("a row's marks: merged but not closed, after a PR, and earlier agents folded", () => {
  assert.deepEqual(marksOf({}), { merged: false, after: null, stalled: null, blocked: [], earlier: new Set() });
  const m = marksOf({
    marks: [
      { kind: "merged_not_closed" },
      { kind: "after", number: 1859, repo: "acme/app" },
      { kind: "earlier", attachment: "att_1" },
      { kind: "earlier", attachment: "att_2" },
    ],
  });
  assert.equal(m.merged, true);
  assert.deepEqual(m.after, { words: "after #1859", repo: "acme/app" });
  assert.deepEqual([...m.earlier], ["att_1", "att_2"]);
  assert.equal(marksOf({ marks: [{ kind: "after", number: 3 }] }).after.repo, undefined);
  assert.equal(earlierLine(1), "earlier agent (1)");
  assert.equal(earlierLine(2), "earlier agents (2)");
});

test("a board counts the asks that wait on the reader, raised ones included, and says who an ask waits on", () => {
  const ask = (over) => ({ id: "ask_1", question: "Ship?", options: [], mine: false, raised: false, can_answer: false, who: "a", who_name: "Sam", asked_by: "an agent on m1", ...over });
  const zones = [
    { rows: [{ id: "row_1", ask: ask({ mine: true }) }, { id: "row_2" }, { id: "row_3", ask: ask({ id: "" , raised: true, mine: true }) }] },
    { rows: [{ id: "row_4", ask: ask({ mine: false }) }] },
  ];
  assert.deepEqual(needsYou({ zones }).map((r) => r.id), ["row_1", "row_3"]);
  assert.deepEqual(needsYou({ zones: [{ rows: [] }] }), []);
  assert.equal(needsYouLine(2), "Needs you (2)");
  assert.equal(askWaitingLine(ask()), "Waiting for Sam, asked by an agent on m1.");
  assert.match(askWaitingLine(ask({ raised: true })), /^Raised by the board/);
  assert.deepEqual([answerReady(""), answerReady("  "), answerReady(" yes ")], [false, false, true]);
});

test("an agent's issue is a chip on a session row and a swarm card, the first and how many more (#372)", () => {
  const on = (issue, ...titles) => ({ issue, boards: titles.map((title, i) => ({ project: P, board: B, title, row: `row_${i}` })) });
  assert.equal(onIssueChip([]), null);
  assert.equal(onIssueChip([null, undefined]), null);
  assert.deepEqual(onIssueChip([null, on("#12", "Launch")]), { label: "#12", title: "#12 on Launch", path: `/p/${P}/boards/${B}#row=row_0` });
  assert.deepEqual(onIssueChip([on("#12")]), { label: "#12", title: "Started on #12", path: null });
  assert.deepEqual(onIssueChip([on("#12", "Launch"), null, on("#13"), on("#14", "Later")]), { label: "#12 +2", title: "#12 on Launch (and 2 more)", path: `/p/${P}/boards/${B}#row=row_0` });
});

test("the swarm's By board groups agents by their first board, and No board goes last (#372)", () => {
  const on = (...titles) => ({ issue: "#1", boards: titles.map((title) => ({ project: P, board: B, title, row: "r" })) });
  assert.equal(boardGroup(on("Launch", "Later")), "Launch");
  assert.equal(boardGroup(on("")), "Untitled board");
  assert.equal(boardGroup(on()), NO_BOARD);
  assert.equal(boardGroup({ issue: "#1" }), NO_BOARD);
  assert.equal(boardGroup(null), NO_BOARD);
  assert.equal(boardGroup(undefined), NO_BOARD);
  // Biggest first, then by name; the pile with no board after them all, however big.
  const names = [[NO_BOARD, 9], ["Zed", 2], ["Alpha", 2], ["Big", 5]];
  assert.deepEqual(names.sort(clusterOrder).map((n) => n[0]), ["Big", "Alpha", "Zed", NO_BOARD]);
});

test("a card's bar is its lane's tone, and a row that waits on the reader takes the attention tone", () => {
  assert.deepEqual(["next", "doing", "review", "done"].map((lane) => cardTone({ lane })), ["muted", "ok", "warn", "ok"]);
  assert.equal(cardTone({ lane: "review", ask: { mine: true } }), "accent");
  assert.equal(cardTone({ lane: "review", ask: { mine: false, raised: true } }), "warn");
  assert.deepEqual(["open", "checks_running", "checks_failed", "merged", "closed", "other"].map(liveTone), ["accent", "warn", "danger", "ok", "muted", "muted"]);
});

test("a group moves one place, and not past either end", () => {
  const g = [{ id: "a" }, { id: "b" }, { id: "c" }];
  assert.deepEqual(groupMove(g, "b", false), { before: "a" });
  assert.deepEqual(groupMove(g, "a", true), { before: "c" });
  assert.deepEqual(groupMove(g, "b", true), { before: null });
  assert.equal(groupMove(g, "a", false), undefined);
  assert.equal(groupMove(g, "c", true), undefined);
  assert.equal(groupMove(g, "zzz", true), undefined);
});

test("the PRs behind a row are its own item if it is one, then the attached ones, once each", () => {
  const pr = (id, number) => ({ pointer: { kind: "forge_item", id }, live: { kind: "pr", repo: "acme/app", number, state: "open", title: "t" } });
  const row = { pointer: { kind: "issue", id: "iss_1" }, live: { kind: "native", number: 1, state: "open", title: "t" }, attachments: [{ role: "work", ...pr("fi_1", 7) }, { role: "work", ...pr("fi_1", 7) }, { role: "proof", ...pr("fi_2", 8) }, { role: "work", pointer: { kind: "link", url: "https://x.test/" } }] };
  assert.deepEqual(prsOf(row).map((p) => p.id), ["fi_1"]);
  assert.deepEqual(prsOf({ ...pr("fi_3", 9), attachments: [] }).map((p) => p.id), ["fi_3"]);
});

test("a check says its conclusion once it has one, and its tone follows", () => {
  const c = (status, conclusion = null) => ({ status, conclusion });
  assert.equal(checkWord(c("completed", "success")), "success");
  assert.equal(checkWord(c("in_progress")), "in progress");
  assert.deepEqual([c("completed", "success"), c("completed", "failure"), c("queued"), c("completed", "skipped")].map(checkTone), ["ok", "danger", "warn", "muted"]);
});

test("a block's branch, pushed state and uncommitted files show only once it has stopped working", () => {
  const facts = { branch: "feat/x", head: "abc1234", pushed: false, uncommitted: 2 };
  assert.deepEqual(settledFacts({ state: "working", ...facts }), []);
  assert.deepEqual(settledFacts({ state: "needs_input", ...facts }), []);
  assert.deepEqual(settledFacts({ state: "idle", ...facts }), ["feat/x@abc1234", "unpushed", "2 uncommitted"]);
  assert.equal(settledFacts({ state: "closed", ...facts }).length, 3);
});

test("a blocked row says what blocks it, or that it can't say, and the board counts the rows that can start", () => {
  const m = marksOf({
    marks: [
      { kind: "blocked_by", number: 7, title: "Design", repo: "acme/app" },
      { kind: "blocked_by" },
    ],
  });
  assert.deepEqual(m.blocked, [{ words: "blocked by #7", title: "Design", repo: "acme/app" }, { words: UNSEEN_BLOCKER }]);
  assert.equal(UNSEEN_BLOCKER, "blocked by something you can't see");
  assert.equal(readyCount({ ready: ["row_1", "row_2"] }), 2);
  assert.equal(readyCount({}), 0);
  const live = { number: 7, title: "Design" };
  assert.equal(relationLine({ kind: "blocks", direction: "in", other: live }), "Blocked by #7 Design");
  assert.equal(relationLine({ kind: "blocks", direction: "out", other: live }), "Blocks #7 Design");
  assert.equal(relationLine({ kind: "duplicate_of", direction: "in" }), "Duplicated by something you can't see");
  assert.equal(relationLine({ kind: "discovered_from", direction: "out", other: live }), "Discovered from #7 Design");
});

test("a stalled row says how long, takes the warn tone, and a block holds its row until it closes or is released", () => {
  assert.deepEqual(["0", "59", "60", "65", "1440", "3000"].map((m) => stalledFor(0, Number(m) * 60_000)), ["0m", "59m", "1h", "1h5m", "1d", "2d"]);
  const m = marksOf({ marks: [{ kind: "stalled", since: 1_000 }] }, 1_000 + 42 * 60_000);
  assert.deepEqual(m.stalled, { words: "stalled 42m", since: 1_000 });
  assert.equal(cardTone({ lane: "doing", marks: [{ kind: "stalled", since: 1 }] }), "warn");
  assert.equal(cardTone({ lane: "doing", marks: [{ kind: "stalled", since: 1 }], ask: { mine: true } }), "warn");
  const block = (state) => ({ role: "work", block: { state }, released: false });
  assert.equal(holdsRow(block("idle")), true);
  assert.equal(holdsRow(block("closed")), false);
  assert.equal(holdsRow({ ...block("working"), released: true }), false);
  assert.equal(holdsRow({ role: "proof", block: { state: "working" } }), false);
  assert.equal(holdsRow({ role: "work" }), false, "a reader who can't see the block sees no claim");
});
