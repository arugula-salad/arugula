// An agent in chat by what it is (Arugula #1582): a board's lead by its
// board, the chief of staff as "Chief of staff", any other by its session,
// never `machine/…`. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { chatAgentName } from "../src/shared/agent-name.ts";

const boards = [
  { title: "Design lift", lead: { machine: "m_dust", session: "Design lift" } },
  { title: "Old", closed_at: 1, lead: { machine: "m_dust", session: "old-work" } },
  { title: "No lead", lead: null },
];

test("a board's lead reads by its board, matched by machine and the session's slug", () => {
  assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust", session: "design-lift" }, boards), "Design lift lead");
  assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust", session: "Design lift" }, boards), "Design lift lead");
  // Another machine's session of that name leads nothing.
  assert.equal(chatAgentName({ machine: "m_geek", machine_name: "geek", session: "design-lift" }, boards), "design-lift");
  // A closed board's lead is its session again.
  assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust", session: "old-work" }, boards), "old-work");
});

test("the chief of staff reads as Chief of staff, any other agent by its session, never its machine", () => {
  for (const session of ["Chief of staff · prj_3e5d80aa", "chief-of-staff-arugula", "cos"]) {
    assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust", session }, boards), "Chief of staff");
  }
  assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust", session: "ux pass" }), "ux pass");
  assert.equal(chatAgentName({ machine: "m_dust", machine_name: "dust" }), "an agent on dust");
  assert.doesNotMatch(chatAgentName({ machine: "m_dust", machine_name: "dust", session: "ux" }), /\//);
});
