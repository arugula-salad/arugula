// The agent panel's pure parts (Lift 4, Arugula #1480): `agentName`, the one
// place a display name comes from; `panelKey`, a tab's key; and the
// transcript cut into prose and tool calls folded to a line. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { agentName, rungOf } from "../src/shared/agent-name.ts";
import { panelKey } from "../src/shared/kit/panel-key.ts";
import { lastLines, partsOf } from "../src/shared/transcript.ts";

test("an agent is named by its work, else its session, else its machine; the chief of staff always by its rung", () => {
  assert.equal(agentName({ role: "worker", title: "#1431 staging checkout", machine: "ws-2", session: "fix" }), "#1431 staging checkout");
  assert.equal(agentName({ role: "lead", title: " ", session: "launch-lead", machine: "dust" }), "launch-lead");
  assert.equal(agentName({ machine: "dust" }), "an agent on dust");
  assert.equal(agentName({}), "An agent");
  assert.equal(agentName({ role: "chief", title: "cos", machine: "dust" }), "Chief of staff");
});

test("as chat tags it, `machine/tab`, or the machine alone", () => {
  assert.equal(agentName({ machine: "dust", session: "fix-installer" }, { handle: true }), "dust/fix-installer");
  assert.equal(agentName({ machine: "dust", session: "" }, { handle: true }), "dust");
  assert.equal(agentName({ machine: "" }, { handle: true }), "a machine that left");
});

test("by role, a Lead or a Worker, wherever a name shows", () => {
  assert.equal(agentName({ role: "lead", title: "Launch" }, { names: "role" }), "Lead");
  assert.equal(agentName({ role: "worker", machine: "dust", session: "x" }, { names: "role", handle: true }), "Worker");
  assert.equal(agentName({}, { names: "role" }), "Agent");
  assert.equal(rungOf({ role: "chief" }), "chief of staff");
  assert.equal(rungOf({}), "agent");
});

test("a tab's key: an agent by machine and session (or its pane), a room, or one message's thread in it", () => {
  assert.equal(panelKey({ kind: "agent", role: "worker", machine: "m_dust", session: "fix", pane: 4 }), "agent:m_dust:fix");
  assert.equal(panelKey({ kind: "agent", role: "agent", machine: "m_dust", pane: 4 }), "agent:m_dust:%4");
  assert.equal(panelKey({ kind: "thread", room: "c_general", title: "#general" }), "thread:c_general");
  assert.equal(panelKey({ kind: "thread", room: "c_general", parent: 0, title: "a thread" }), "thread:c_general:0");
});

const TRANSCRIPT = [
  "## You",
  "",
  "Fix the checkout.",
  "",
  "Reading the **checkout** first: `stripe.ts`.",
  "",
  "**Read** `web/src/billing/checkout.ts` (completed)",
  "",
  "```",
  "export const x = 1;",
  "",
  "more",
  "```",
  "",
  "**Execute** `npm test` (failed, exit 1)",
  "",
  "    $ npm test -- --run",
  "",
  "It fails: STRIPE_KEY isn't set.",
].join("\n");

test("a transcript is prose and tool calls, each call one line with what hung under it", () => {
  const parts = partsOf(TRANSCRIPT);
  assert.deepEqual(
    parts.map((p) => (p.kind === "tool" ? `tool ${p.line}` : `prose ${p.text.split("\n")[0]}`)),
    ["prose ## You", "tool **Read** `web/src/billing/checkout.ts` (completed)", "tool **Execute** `npm test` (failed, exit 1)", "prose It fails: STRIPE_KEY isn't set."],
  );
  assert.equal(parts[1].body, "```\nexport const x = 1;\n\nmore\n```");
  assert.equal(parts[2].body, "    $ npm test -- --run");
  assert.ok(parts[0].text.endsWith("`stripe.ts`."));
});

test("what was said leaves the tool calls out", () => {
  assert.deepEqual(
    partsOf(TRANSCRIPT, "said").map((p) => p.kind),
    ["prose", "prose"],
  );
});

test("the last lines reopen a fence they cut into", () => {
  const all = ["a", "```", "b", "c", "```", "d"];
  assert.deepEqual(lastLines(all, true, 3), ["```", "c", "```", "d"]);
  assert.deepEqual(lastLines(all, false, 2), ["```", "d"]);
  assert.deepEqual(lastLines(all, true, 10), all);
});
