// A project's Overview (Arugula inversion, L6): its boards with cards in
// flight, the most urgent first; each card's sessions, found by an agent's
// `on_issue`, a row's attachment, the hub's sessions by card or the pane's
// session; and Not on a card last. And the hub's by-card route, 404 being
// "not yet". Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { boardLine, overviewOf, statsLine } from "../src/shell/swarm/overview.ts";
import { panesOf, sessionsByCard } from "../src/shared/project-sessions.ts";
import { BY_CARD, PROJECT } from "./project-sessions.fixture.ts";

const P = PROJECT;
const row = (id, lane, n, extra = {}) => ({ id, lane, rev: 1, pointer: { kind: "issue", id: `iss_${n}` }, note: "", attachments: [], live: { state: "open", kind: "native", number: n, title: `Card ${n}` }, updated_at: n, ...extra });
const view = (id, title, rows, extra = {}) => ({
  board: { id, title, project: P, ...extra },
  zones: ["next", "doing", "review", "done"].map((l) => ({ id: `lane_${l}`, title: l, rows: rows.filter((r) => r.lane === l) })),
  groups: [],
  docs: [],
});
const pane = (host, id, extra = {}) => ({ key: `${host}:${id}`, host, id, session: { id: 1, name: "main" }, project: P, on_issue: null, name: `pane ${id}`, agent: false, standing: "quiet", last: 0, ...extra });
const hosts = { m_ws: "workstation", m_b1: "build-01" };
const hostOf = (m) => hosts[m];

test("only Doing and Review are drawn, Next and Done are counts, and a board with nothing in flight isn't there", () => {
  const launch = view("brd_l", "Launch", [row("r1", "next", 1), row("r2", "doing", 2), row("r3", "review", 3), row("r4", "done", 4), row("r5", "done", 5)]);
  const later = view("brd_x", "Later", [row("r6", "next", 6)]);
  const o = overviewOf(P, [launch, later], [], null, hostOf);
  assert.deepEqual(o.boards.map((b) => b.title), ["Launch"]);
  const b = o.boards[0];
  assert.deepEqual([b.doing.map((c) => c.row.id), b.review.map((c) => c.row.id)], [["r2"], ["r3"]]);
  assert.equal(boardLine(b), "2 in flight · 1 next · 2 of 5 done");
  // A card no one is on.
  assert.equal(b.doing[0].standing, "idle");
});

test("a closed board isn't drawn", () => {
  const o = overviewOf(P, [view("brd_c", "Shipped", [row("r1", "doing", 1)], { closed_at: 5 })], [], null, hostOf);
  assert.equal(o.boards.length, 0);
});

test("cards that need you come first, then failures, then quiet ones, then working ones; boards by their most urgent card", () => {
  const rows = [row("rw", "doing", 1), row("rq", "doing", 2), row("rf", "doing", 3), row("rn", "doing", 4)];
  const on = (r, n) => ({ issue: `#${n}`, boards: [{ project: P, board: "brd_a", title: "A", row: r }] });
  const panes = [
    pane("workstation", 1, { on_issue: on("rw", 1), standing: "working", agent: true }),
    pane("workstation", 2, { on_issue: on("rq", 2), standing: "quiet", agent: true, session: { id: 2, name: "s2" } }),
    pane("workstation", 3, { on_issue: on("rf", 3), standing: "failed", agent: true, session: { id: 3, name: "s3" } }),
    pane("workstation", 4, { on_issue: on("rn", 4), standing: "needs", agent: true, session: { id: 4, name: "s4" } }),
    pane("workstation", 5, { on_issue: { issue: "#9", boards: [{ project: P, board: "brd_b", title: "B", row: "rb" }] }, standing: "failed", session: { id: 5, name: "s5" } }),
  ];
  const o = overviewOf(P, [view("brd_b", "B", [row("rb", "review", 9)]), view("brd_a", "A", rows)], panes, null, hostOf);
  assert.deepEqual(o.boards.map((b) => b.title), ["A", "B"]);
  assert.deepEqual(o.boards[0].doing.map((c) => c.row.id), ["rn", "rf", "rq", "rw"]);
  assert.deepEqual(o.boards[0].doing.map((c) => c.standing), ["needs", "failed", "quiet", "working"]);
  assert.equal(statsLine(o, 1), "5 cards in flight · 1 session working · 1 needs you");
});

test("a card the board marks stalled is quiet, though a session on it works", () => {
  const r = row("r1", "doing", 1, { marks: [{ kind: "stalled" }] });
  const p = pane("workstation", 1, { on_issue: { issue: "#1", boards: [{ project: P, board: "brd_a", title: "A", row: "r1" }] }, standing: "working" });
  assert.equal(overviewOf(P, [view("brd_a", "A", [r])], [p], null, hostOf).boards[0].doing[0].standing, "quiet");
});

test("a pane finds its card by its agent's card number, by a row's attachment, and by its session", () => {
  const rows = [row("r1", "doing", 1), row("r2", "doing", 2, { attachments: [{ id: "a", role: "work", pointer: { kind: "block", machine: "m_b1", pane: "%7" } }] })];
  const panes = [
    // Started on #1, with no board named.
    pane("workstation", 1, { on_issue: { issue: "#1", project: P }, agent: true, name: "Claude · #1" }),
    // Its terminal, in the same session, has no card of its own.
    pane("workstation", 2, { name: "cargo test" }),
    // Attached to r2 by the row.
    pane("build-01", 7, { session: { id: 9, name: "i2" }, agent: true }),
    // Another project's, on no card: not this Overview's.
    pane("build-01", 8, { project: "prj_other", session: { id: 3, name: "x" } }),
  ];
  const o = overviewOf(P, [view("brd_a", "A", rows)], panes, null, hostOf);
  const [c1, c2] = [...o.boards[0].doing].sort((a, b) => a.row.id.localeCompare(b.row.id));
  assert.equal(c1.sessions.length, 1);
  assert.deepEqual(c1.sessions[0].panes.map((p) => p.key), ["workstation:1", "workstation:2"]);
  // The agent leads its session's tile, and names it.
  assert.equal(c1.sessions[0].lead.key, "workstation:1");
  assert.equal(c1.sessions[0].name, "Claude · #1");
  assert.equal(c1.sessions[0].where, "workstation");
  assert.deepEqual(c2.sessions.map((s) => s.lead.key), ["build-01:7"]);
  assert.equal(o.cardOf.has("build-01:8"), false);
  assert.equal(o.loose.length, 0);
});

test("a terminal in an agent's session is on the agent's card, though its session names no project", () => {
  const on = { issue: "#1", boards: [{ project: P, board: "brd_a", title: "A", row: "r1" }] };
  const panes = [pane("workstation", 1, { project: null, on_issue: on, agent: true }), pane("workstation", 2, { project: null })];
  const o = overviewOf(P, [view("brd_a", "A", [row("r1", "doing", 1)])], panes, null, hostOf);
  assert.deepEqual(o.boards[0].doing[0].sessions[0].panes.map((p) => p.key), ["workstation:1", "workstation:2"]);
});

test("a session's tile is named by its agent, and opens the pane that asks", () => {
  const on = { issue: "#1", boards: [{ project: P, board: "brd_a", title: "A", row: "r1" }] };
  const panes = [pane("workstation", 1, { on_issue: on, agent: true, name: "Claude Code · #1 docs" }), pane("workstation", 2, { standing: "needs", name: "bash" })];
  const s = overviewOf(P, [view("brd_a", "A", [row("r1", "doing", 1)])], panes, null, hostOf).boards[0].doing[0].sessions[0];
  assert.equal(s.name, "Claude Code · #1 docs");
  assert.equal(s.lead.key, "workstation:2");
  assert.equal(s.standing, "needs");
});

test("Not on a card: the project's sessions that work on none, each a tile", () => {
  const panes = [pane("workstation", 1, { name: "Lead agent", agent: true, session: { id: 1, name: "Project Inversion" } }), pane("workstation", 2, { session: { id: 2, name: "scratch" }, name: "just check" }), pane("workstation", 3, { session: { id: 2, name: "scratch" } })];
  const o = overviewOf(P, [], panes, null, hostOf);
  assert.equal(o.boards.length, 0);
  // A session of two terminals is named by its session.
  assert.deepEqual(o.loose.map((s) => s.name).sort(), ["Lead agent", "scratch"]);
  assert.equal(o.cardOf.get("workstation:2"), null);
});

test("the hub's sessions by card place panes the machines report, and name the sessions they don't", () => {
  const rows = [row("r983", "doing", 983), row("r987", "review", 987)];
  const panes = [pane("workstation", 12, { session: { id: 7, name: "i983" }, project: null, agent: true, standing: "needs" }), pane("workstation", 13, { session: { id: 7, name: "i983" }, project: null }), pane("build-01", 4, { session: { id: 5, name: "s" }, project: null, agent: true })];
  const o = overviewOf(P, [view("brd_a", "A", rows)], panes, BY_CARD, hostOf);
  const c983 = o.boards[0].doing[0];
  // The machine's session, with both its panes, and the cloud workspace's, which has no pane here.
  assert.deepEqual(c983.sessions.map((s) => [s.name, s.where, s.panes.length]), [["pane 12", "workstation", 2], ["arugula/983-cloud-half", "Arugula cloud", 0]]);
  assert.equal(c983.sessions[0].lead.key, "workstation:12");
  assert.equal(c983.sessions[1].hosted, "hs_1");
  assert.equal(c983.standing, "needs");
  assert.deepEqual(o.boards[0].review[0].sessions.map((s) => s.lead?.key), ["build-01:4"]);
  // A machine this page doesn't reach: its session is named, on no card.
  assert.deepEqual(o.loose.map((s) => [s.name, s.where]), [["release", "laptop"]]);
});

test("the hub's route: 404 is not yet, an answer is read, anything else throws", async () => {
  const answer = (status, body) => async () => new Response(JSON.stringify(body), { status });
  assert.equal(await sessionsByCard(P, answer(404, { error: "not found" })), null);
  assert.deepEqual(await sessionsByCard(P, answer(200, BY_CARD)), BY_CARD);
  await assert.rejects(sessionsByCard(P, answer(500, { error: "down" })), /down/);
  let asked = "";
  await sessionsByCard("prj_a b", async (url) => ((asked = url), new Response("{}", { status: 404 })));
  assert.equal(asked, "/api/projects/prj_a%20b/sessions/by-card");
});

test("a group's panes: an agent block is its own pane, a session lists its panes, a cloud workspace's has none", () => {
  assert.deepEqual(panesOf(BY_CARD.groups[0].sessions), [{ machine: "m_ws", pane: 12 }, { machine: "m_ws", pane: 13 }]);
  assert.deepEqual(panesOf(BY_CARD.groups[1].sessions), [{ machine: "m_b1", pane: 4 }]);
});
