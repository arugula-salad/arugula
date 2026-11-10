// A board's words and paths (boards, step 2): a row's glyph, a
// zone's progress, a pointer's label and the id a person types for one, and
// the board routes' paths. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { closedWithLine, holdsRow, stalledFor, settledFacts, LANE_GLYPH, cardTone, checkTone, checkWord, groupMove, liveTone, prsOf, answerReady, askWaitingLine, assigneesLine, chipOf, counted, debounced, earlierLine, epicLine, firstLine, groupLine, isCard, laneCount, liveWord, marksOf, readyCount, relationLine, UNSEEN_BLOCKER, needsYou, needsYouLine, parsePointer, pointerHref, pointerLabel, pointerToSend, proofMark, rowGlyph, sections, stepActive, workLabel, workChip, onIssueChip, boardGroup, clusterOrder, NO_BOARD, sourceLine, sourceLinked, leftOutLine, docWords, docHref, onBoardWords, pointerText, doneRefusal, laneOfZone, laneSetLine, closeName, rowMatches, findLine, boardMatches, rowHasLabel, rowHasPerson, rowPasses, filtering, filterChoices, filterHash, parseFilterHash, topAndMore, initialsOf } from "../src/shared/boards/text.ts";
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
  const z = { review: 0, doing: 0 };
  assert.deepEqual(epicLine({ done: 3, total: 5, proven: 2, ...z }), {
    words: "3 of 5 done, 2 proven",
    short: "3 of 5 done",
    pct: 60,
    segments: [{ lane: "done", pct: 60 }],
  });
  assert.deepEqual(epicLine({ done: 0, total: 2, proven: 0, ...z }), { words: "0 of 2 done", short: "0 of 2 done", pct: 0, segments: [] });
  assert.deepEqual(epicLine({ done: 0, total: 0, proven: 0, ...z }), { words: "No work yet", short: "No work yet", pct: 0, segments: [] });
  // Every part says its count; a part with none is left out, of the words and of the bar.
  assert.deepEqual(epicLine({ done: 3, total: 10, proven: 0, review: 2, doing: 1 }), {
    words: "3 of 10 done, 0 proven · 2 in review · 1 doing",
    short: "3 of 10 done",
    pct: 30,
    segments: [{ lane: "done", pct: 30 }, { lane: "review", pct: 20 }, { lane: "doing", pct: 10 }],
  });
  assert.deepEqual(epicLine({ done: 0, total: 4, proven: 0, review: 0, doing: 1 }), {
    words: "0 of 4 done · 1 doing",
    short: "0 of 4 done",
    pct: 0,
    segments: [{ lane: "doing", pct: 25 }],
  });
  assert.equal(epicLine({ done: 1, total: 4, proven: 1, review: 3, doing: 0 }).words, "1 of 4 done, 1 proven · 3 in review");
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
  assert.deepEqual(proofMark({ lane: "done", pointer: issue, proof: { by: "ci" } }), { proven: true, words: "Proof", title: "proven by CI" });
  assert.deepEqual(proofMark({ lane: "done", pointer: issue, proof: { by: "evidence" } }), { proven: true, words: "Proof", title: "proven" });
  assert.deepEqual(proofMark({ lane: "done", pointer: issue }), { proven: false, words: "no proof", title: "not proven yet" });
  assert.equal(proofMark({ lane: "review", pointer: issue }), null);
  // A cancelled row says so in its mark and is not "not proven yet".
  assert.equal(proofMark({ lane: "done", pointer: issue, marks: [{ kind: "cancelled" }] }), null);
  assert.equal(marksOf({ marks: [{ kind: "cancelled" }] }).cancelled, true);
  // Which PR closed a row's issue, and by which words (Arugula #743).
  assert.equal(closedWithLine({ closed_with: "acme/site#70", closed_by: "\"Closes #4\" in the body" }), "closed with acme/site#70, by \"Closes #4\" in the body");
  assert.equal(closedWithLine({ closed_with: "acme/site#70" }), "closed with acme/site#70");
  assert.equal(closedWithLine({}), null);
  assert.equal(closedWithLine(undefined), null);
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
  assert.deepEqual(chipOf({ role: "work", pointer: { kind: "block", machine: "m", pane: "7" }, block: { state: "working" } }), { label: "agent", state: null }, "the Doing lane says working");
  assert.equal(chipOf({ role: "work", pointer: { kind: "block", machine: "m", pane: "7" }, block: { state: "needs_input" } }).state, "needs input");
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
  assert.deepEqual(got[0].lanes.map((z) => z.id), ["lane_doing", "lane_review", "lane_next", "lane_done"], "Doing, Review, Next, Done (#479)");
  // Nothing ungrouped: no last section. A row in a group that's gone is ungrouped.
  assert.equal(sections({ zones: lanes.map((z) => ({ ...z, rows: z.rows.filter((r) => r.group) })), groups }).length, 2);
  assert.equal(sections({ zones: [{ id: "lane_next", title: "Next", rows: [row("x", "zon_9")] }], groups }).at(-1).lanes[0].rows[0].id, "x");
  // No groups (or none from an old platform): the lanes in the board's order.
  const ordered = ["lane_doing", "lane_review", "lane_next", "lane_done"].map((id) => lanes.find((z) => z.id === id));
  assert.deepEqual(sections({ zones: lanes, groups: [] }), [{ group: null, lanes: ordered }]);
  assert.deepEqual(sections({ zones: lanes }), [{ group: null, lanes: ordered }]);
  assert.equal(groupLine({ done: 1, total: 2 }), "1 of 2 done");
  assert.equal(groupLine({ done: 0, total: 0 }), "no work yet");
});

test("a row's marks: merged but not closed, after a PR, and earlier agents folded", () => {
  assert.deepEqual(marksOf({}), { merged: false, cancelled: false, after: null, stalled: null, blocked: [], earlier: new Set() });
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

test("a card's bar is its lane's tone, the progress bar's, and a row that waits on the reader keeps it", () => {
  assert.deepEqual(["next", "doing", "review", "done"].map((lane) => cardTone({ lane })), ["muted", "warn", "accent", "ok"]);
  assert.deepEqual(["next", "doing", "review", "done"].map((lane) => cardTone({ lane, ask: { mine: true } })), ["muted", "warn", "accent", "ok"]);
  assert.equal(cardTone({ lane: "review", ask: { mine: false, raised: true } }), "accent");
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

test("a stalled row says how long, takes the danger tone, and a block holds its row until it closes or is released", () => {
  assert.deepEqual(["0", "59", "60", "65", "1440", "3000"].map((m) => stalledFor(0, Number(m) * 60_000)), ["0m", "59m", "1h", "1h5m", "1d", "2d"]);
  const m = marksOf({ marks: [{ kind: "stalled", since: 1_000 }] }, 1_000 + 42 * 60_000);
  assert.deepEqual(m.stalled, { words: "stalled 42m", since: 1_000 });
  assert.equal(cardTone({ lane: "doing", marks: [{ kind: "stalled", since: 1 }] }), "danger");
  assert.equal(cardTone({ lane: "doing", marks: [{ kind: "stalled", since: 1 }], ask: { mine: true } }), "danger");
  const block = (state) => ({ role: "work", block: { state }, released: false });
  assert.equal(holdsRow(block("idle")), true);
  assert.equal(holdsRow(block("closed")), false);
  assert.equal(holdsRow({ ...block("working"), released: true }), false);
  assert.equal(holdsRow({ role: "proof", block: { state: "working" } }), false);
  assert.equal(holdsRow({ role: "work" }), false, "a reader who can't see the block sees no claim");
});

test("folds: everything starts open, a toggle is kept per board, and a bad or blocked store reads as none (#480)", async () => {
  const { FOLD_DONE_BY_DEFAULT, groupFold, groupLaneFold, isFolded, laneFold, loadFolds, saveFolds } = await import("../src/shared/boards/folds.ts");
  const mem = new Map();
  globalThis.localStorage = { getItem: (k) => mem.get(k) ?? null, setItem: (k, v) => void mem.set(k, v) };
  assert.equal(FOLD_DONE_BY_DEFAULT, false);
  assert.equal(isFolded({}, groupFold("zon_1")), false);
  assert.equal(isFolded({}, laneFold("lane_done")), false);
  assert.equal(groupLaneFold("zon_1", "lane_done"), "lane:zon_1:lane_done");
  assert.equal(groupLaneFold(null, "lane_done"), "lane:none:lane_done");
  assert.notEqual(groupLaneFold("zon_1", "lane_done"), laneFold("lane_done"));
  const inA = { [groupLaneFold("zon_1", "lane_done")]: true };
  assert.equal(isFolded(inA, groupLaneFold("zon_1", "lane_done")), true);
  assert.equal(isFolded(inA, groupLaneFold("zon_2", "lane_done")), false, "another group's lane is untouched");
  assert.equal(isFolded(inA, groupLaneFold(null, "lane_done")), false);
  assert.equal(isFolded(inA, laneFold("lane_done")), false);
  saveFolds("brd_a", { [groupFold("zon_1")]: true, [laneFold("lane_next")]: false });
  assert.deepEqual(loadFolds("brd_a"), { "group:zon_1": true, "lane:lane_next": false });
  assert.deepEqual(loadFolds("brd_b"), {}, "another board is untouched");
  mem.set("arugula.board.folds.brd_c", "not json");
  assert.deepEqual(loadFolds("brd_c"), {});
  mem.set("arugula.board.folds.brd_d", JSON.stringify({ a: true, b: "yes", c: 1 }));
  assert.deepEqual(loadFolds("brd_d"), { a: true });
  globalThis.localStorage = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  assert.deepEqual(loadFolds("brd_a"), {});
  saveFolds("brd_a", { x: true });
  delete globalThis.localStorage;
});

test("a board from a Plane epic says where from, links only an https url, and counts what it left out", () => {
  assert.equal(sourceLine({ key: "ARUGU-10" }), "From Plane · ARUGU-10");
  assert.equal(sourceLinked({ url: "https://app.plane.so/a/browse/ARUGU-10/" }), true);
  assert.equal(sourceLinked({ url: "javascript:alert(1)" }), false);
  assert.equal(sourceLinked({ url: "" }), false);
  assert.equal(leftOutLine(1), "1 done child left out");
  assert.equal(leftOutLine(3), "3 done children left out");
});

test("a Doc's chip says its written sections, or the machine it is on; only work is a PR", () => {
  const doc = { kind: "doc", id: "pdc_0123456789abcdef0123456789abcdef" };
  assert.equal(docWords({ pointer: doc, sections: 5, written: 3 }), "3/5");
  assert.equal(docWords({ pointer: doc, sections: 2 }), "0/2");
  assert.equal(docWords({ pointer: doc, sections: 0, written: 0 }), "", "a Doc with no sections says nothing");
  assert.equal(docWords({ pointer: doc, missing: true }), "");
  const machine = { kind: "machine_doc", machine: "m1", doc: "00112233445566ff", title: "Perf notes" };
  assert.equal(docWords({ pointer: machine, machine_name: "geek-box" }), "on geek-box");
  assert.equal(docWords({ pointer: machine }), "on a machine");
  assert.equal(docWords({ pointer: machine, machine_name: "geek-box", sections: 2, written: 1 }), "1/2 on geek-box", "a machine Doc with a copy has counts");
  assert.equal(pointerLabel(machine), "Perf notes");
  assert.equal(pointerLabel(doc), "Doc 01234567");
  assert.equal(pointerText(machine), "", "a machine Doc is never typed in");
  assert.equal(docHref(P, doc.id, (path) => `https://x.test${path}`), `https://x.test/p/${P}/docs/${doc.id}`);
  assert.equal(docHref(P, doc.id, () => undefined), undefined);
  assert.equal(onBoardWords({ strip: true, rows: 0 }), "Docs strip");
  assert.equal(onBoardWords({ strip: false, rows: 1 }), "1 row");
  assert.equal(onBoardWords({ strip: true, rows: 2 }), "Docs strip, 2 rows");
  // Only work is a row's PR: proof is none.
  const pr = { kind: "forge_item", id: "fi_1" };
  const live = { state: "open", kind: "pr", repo: "a/b", number: 1, title: "t" };
  const row = { pointer: { kind: "none" }, live: null, attachments: [{ id: "att_1", role: "proof", pointer: { kind: "forge_item", id: "fi_9" }, live }, { id: "att_2", role: "work", pointer: pr, live }] };
  assert.deepEqual(prsOf(row).map((p) => p.id), ["fi_1"]);
});

test("moving a row by hand: Done closes a native issue only, and a set lane says who set it", () => {
  assert.equal(doneRefusal({ pointer: { kind: "issue", id: "iss_1" } }), null);
  assert.equal(doneRefusal({ pointer: { kind: "forge_item", id: "fi_1" } }), "Close it in GitHub/Plane to move it to Done.");
  assert.equal(doneRefusal({ pointer: { kind: "none" } }), "A row with no issue can't be moved to Done.");
  assert.equal(laneOfZone("lane_review"), "review");
  assert.equal(laneOfZone("zon_abc"), null);
  assert.equal(closeName({ live: { number: 12 } }), "#12");
  assert.equal(closeName({}), "this issue");
  const set = { lane_set: { lane: "next", by: "acc_1", at: 1 } };
  assert.equal(laneSetLine(set, () => "Kim"), "set by Kim");
  assert.equal(laneSetLine(set, () => undefined), "set by someone who left");
  assert.equal(laneSetLine({}, () => "Kim"), null);
});

test("a find box matches a row by number, tracker key or words, and every word has to hold (#546)", () => {
  const row = (title, number, extra = {}, note = "") => ({ pointer: { kind: "issue", id: "iss_1" }, note, live: { title, number, kind: "native", state: "open", ...extra } });
  const a = row("Search the board", 541);
  const b = row("Plane epics fold", 12, { identifier: "ARUGU", provider: "plane" }, "folded lanes unfold on a match");
  const note = { pointer: { kind: "none" }, note: "Remember the Launch checklist" };
  assert.equal(rowMatches(a, ""), true, "an empty find keeps every row");
  assert.equal(rowMatches(a, "  "), true);
  assert.equal(rowMatches(a, "#541"), true);
  assert.equal(rowMatches(a, "541"), true);
  assert.equal(rowMatches(a, "#54"), false, "a number is the whole number, not a prefix");
  assert.equal(rowMatches(a, "#12"), false);
  assert.equal(rowMatches(b, "arugu-12"), true, "a tracker key, in any case");
  assert.equal(rowMatches(b, "ARUGU-13"), false);
  assert.equal(rowMatches(a, "ARUGU-541"), false, "a native issue has no tracker key");
  assert.equal(rowMatches(a, "SEARCH BOARD"), true, "words in any order, any case");
  assert.equal(rowMatches(a, "search lane"), false, "every word has to appear");
  assert.equal(rowMatches(b, "unfold match"), true, "the note is read");
  assert.equal(rowMatches(b, "#12 unfold"), true, "a number and a word together");
  assert.equal(rowMatches(b, "#13 unfold"), false);
  assert.equal(rowMatches(note, "launch"), true, "a note row has its note");
  assert.equal(rowMatches(note, "#1"), false);
  assert.equal(rowMatches(row("Search", 1, { labels: ["boards"] }), "boards"), true, "plain words read the labels");
  assert.equal(rowMatches(row("Run", 0), "#0"), false, "a run has no number");
  assert.equal(findLine(7, 89), "7 of 89 rows");
  assert.equal(findLine(1, 1), "1 of 1 row");
});

test("a boards list filters by title, epic key or goal, open and closed alike (#546)", () => {
  const b = { title: "Launch", goal: "Ship the editor", source: { key: "ARUGU-10" } };
  assert.equal(boardMatches(b, ""), true);
  assert.equal(boardMatches(b, "launch"), true);
  assert.equal(boardMatches(b, "arugu-10"), true);
  assert.equal(boardMatches(b, "ship editor"), true);
  assert.equal(boardMatches(b, "ship docs"), false);
  assert.equal(boardMatches({ title: "Done one", goal: "", source: undefined }, "arugu"), false);
});

test("label: and @ in the find box are a label and a person; the menus are OR within and AND across (#547)", () => {
  const row = (title, labels, assignees) => ({ pointer: { kind: "issue", id: "iss_1" }, note: "", live: { title, number: 4, kind: "native", state: "open", labels, assignees } });
  const a = row("Search", ["boards", "ui"], ["Ana Diaz"]);
  const b = row("Fold", ["docs"], ["bob"]);
  const c = row("Bare", undefined, undefined);
  assert.equal(rowMatches(a, "label:boards"), true);
  assert.equal(rowMatches(a, "LABEL:UI"), true, "any case");
  assert.equal(rowMatches(a, "label:board"), false, "a label is whole");
  assert.equal(rowMatches(b, "label:boards"), false);
  assert.equal(rowMatches(c, "label:boards"), false, "no labels, no match");
  assert.equal(rowMatches(a, "@ana"), true, "part of a name");
  assert.equal(rowMatches(a, "@ana_diaz"), true, "a space reads as _");
  assert.equal(rowMatches(a, "@bob"), false);
  assert.equal(rowMatches(c, "@bob"), false);
  assert.equal(rowMatches(a, "label:boards @ana search"), true, "words together");
  assert.equal(rowMatches(a, "label:boards @bob"), false, "every word has to hold");
  assert.equal(rowMatches(a, "label:"), false, "a bare label: is text");
  assert.equal(rowHasLabel(a, " Boards "), true);
  assert.equal(rowHasPerson(a, ""), false);
  const f = (labels, people, q = "") => ({ q, labels, people });
  assert.deepEqual([a, b, c].map((r) => rowPasses(r, f([], []))), [true, true, true], "no filter keeps all");
  assert.deepEqual([a, b, c].map((r) => rowPasses(r, f(["boards", "docs"], []))), [true, true, false], "any label of the menu");
  assert.deepEqual([a, b, c].map((r) => rowPasses(r, f(["boards", "docs"], ["bob"]))), [false, true, false], "the menus both hold");
  assert.deepEqual([a, b, c].map((r) => rowPasses(r, f(["boards", "docs"], ["bob", "ana"], "fold"))), [false, true, false], "and the text");
  assert.equal(filtering(f([], [])), false);
  assert.equal(filtering(f([], [], " x ")), true);
  assert.equal(filtering(f(["a"], [])), true);
  assert.deepEqual(filterChoices([a, b, row("x", ["Docs", "ui"], ["bob"]), c], "labels"), [
    { value: "docs", count: 2 }, { value: "ui", count: 2 }, { value: "boards", count: 1 },
  ].sort((x, y) => y.count - x.count || x.value.localeCompare(y.value)), "most common first, each once");
  assert.deepEqual(filterChoices([a, b], "assignees").map((x) => x.value), ["Ana Diaz", "bob"]);
});

test("a filter lives in the URL hash, and the chips fold into +N (#547)", () => {
  const f = { q: "fold", labels: ["boards", "needs work"], people: ["Ana Diaz"] };
  const h = filterHash(f);
  assert.equal(h, "q=fold&label=boards&label=needs+work&person=Ana+Diaz");
  assert.deepEqual(parseFilterHash("#" + h), f);
  assert.equal(filterHash({ q: " ", labels: [], people: [] }), "");
  assert.equal(parseFilterHash(""), null);
  assert.equal(parseFilterHash("#other=1"), null, "a hash that names no filter is none");
  assert.deepEqual(parseFilterHash("#label=x&label="), { q: "", labels: ["x"], people: [] });
  assert.deepEqual(topAndMore(["a", "b", "c", "d", "e"], 3), { top: ["a", "b", "c"], more: 2 });
  assert.deepEqual(topAndMore(["a"], 3), { top: ["a"], more: 0 });
  assert.equal(initialsOf("Ana Diaz"), "AD");
  assert.equal(initialsOf("alice"), "A");
  assert.equal(initialsOf("bob-the_builder x"), "BT");
  assert.equal(initialsOf(""), "?");
});

// The Steps view (Arugula #763): a reorder keeps to its group's order, a link is an arrow the way its kind runs.
import { lastPlace, linksOf, loadMode, nudge, phases, reorder, shown, stepPlace } from "../src/shared/boards/steps.ts";

test("Steps is the default at 390 px and Lanes elsewhere, and a reorder names the whole group's order it read", () => {
  assert.equal(loadMode(390), "steps");
  assert.equal(loadMode(1280), "lanes");
  // One group, a b c d e; the step is a, c, d (b and e are in other steps).
  const group = ["a", "b", "c", "d", "e"];
  const step = ["a", "c", "d"];
  // d before a: the whole order is what was read, `before` is a.
  assert.deepEqual(reorder(group, step, "d", "a"), { before: "a", seen: group });
  // a to the end of its step goes before the next row of the group after the step's last, wherever that is.
  assert.deepEqual(reorder(group, step, "a", null), { before: "e", seen: group });
  // Where it already is, nothing is sent.
  assert.equal(reorder(group, step, "c", "d"), null);
  assert.equal(reorder(group, step, "d", null), null);
  assert.deepEqual(nudge(group, step, "c", true), { before: "a", seen: group });
  assert.deepEqual(nudge(group, step, "c", false), { before: "e", seen: group });
  assert.equal(nudge(group, step, "a", true), null);
  // Step 1 folds after 8.
  assert.deepEqual(shown(1, [...Array(11).keys()], false), { rows: [...Array(8).keys()], more: 3 });
  assert.equal(shown(2, [...Array(11).keys()], false).more, 0);
});

test("phases hold every row by place, done rows first inside a place, and count the done ones; links run the way their kind does", () => {
  const row = (id, lane, step, group, item, relations, place) => ({ id, lane, step, place, group, pointer: { kind: "issue", id: item }, relations });
  const g = (id) => ({ id, title: id });
  // r3 is done and sits in place 1 beside r1; r2 is ready (step 1) but sits in place 2, after the done r3's chain; r5 has no place, so its step is its column.
  const rows = [row("r1", "next", 1, "g1", "iss_1", undefined, 1), row("r2", "next", 1, "g1", "iss_2", undefined, 2), row("r3", "done", undefined, "g1", "iss_3", undefined, 1), row("r4", "next", 1, undefined, "iss_4", undefined, 1), row("r5", "next", 2, undefined, "iss_5")];
  const v = { groups: [g("g1")], zones: [{ rows }], order: ["r2", "r1", "r3", "r4", "r5"], sequence: ["r1", "r2", "r4"] };
  const ps = phases(v);
  assert.deepEqual(ps.map((p) => [p.group?.id ?? null, p.done, p.places.map((s) => [s.place, s.rows.map((r) => r.id)])]), [["g1", 1, [[1, ["r3", "r1"]], [2, ["r2"]]]], [null, 0, [[1, ["r4"]], [2, ["r5"]]]]]);
  assert.equal(lastPlace(ps), 2);
  // A done row can't be moved, and an open row moves among the open rows of its place.
  assert.equal(stepPlace(v, "r3"), null);
  assert.deepEqual(stepPlace(v, "r1")?.step, ["r1"]);
  const rel = (id, kind, direction, item) => ({ id, kind, direction, item });
  const me = row("r1", "next", 1, "g1", "iss_1", [rel("a", "before", "out", "iss_2"), rel("b", "discovered_from", "out", "iss_4"), rel("c", "relates", "in", "iss_3")]);
  const drawn = new Set(["r1", "r2", "r4"]);
  // r3 is done and not on screen here, so its link is left out; "discovered from" runs from where it was found.
  assert.deepEqual(linksOf(me, [me, ...rows.slice(1)], drawn).map((l) => [l.kind, l.from, l.to]), [["before", "r1", "r2"], ["discovered_from", "r4", "r1"]]);
});
