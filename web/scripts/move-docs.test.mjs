// Moving Docs into a team (Arugula #818): what each Doc's outcome is, against
// a fake platform. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { moveDocsToTeam } from "../src/shared/docs/move-docs.ts";

const doc = (id, title, machine = null) => ({ id, title, machine });
const json = (status, body) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

/** A platform answering by "METHOD path"; the calls it got are in `calls`. */
function platform(routes) {
  const calls = [];
  globalThis.fetch = async (path, init = {}) => {
    const key = `${init.method ?? "GET"} ${path}`;
    calls.push({ key, body: init.body ? JSON.parse(init.body) : undefined });
    const answer = routes[key];
    if (!answer) throw new Error(`no route ${key}`);
    return typeof answer === "function" ? answer() : answer;
  };
  return calls;
}

test("a machine's copy is made in the team; a 409 with the team Doc's id is already there; a refusal says why", async () => {
  const calls = platform({
    "POST /api/projects/prj_me/docs/a/copies": json(200, { doc: { id: "t_a" } }),
    "POST /api/projects/prj_me/docs/b/copies": json(409, { error: "already shared", id: "t_b" }),
    "POST /api/projects/prj_me/docs/c/copies": json(403, { error: "You can't write there" }),
  });
  const seen = [];
  const out = await moveDocsToTeam("prj_me", "prj_team", [doc("a", "A", "dev_1"), doc("b", "B", "dev_1"), doc("c", "C", "dev_1")], (r, n, of) => seen.push([r.id, n, of]));
  assert.deepEqual(out, [
    { id: "a", title: "A", outcome: "made", teamDoc: "t_a" },
    { id: "b", title: "B", outcome: "there", teamDoc: "t_b" },
    { id: "c", title: "C", outcome: "refused", why: "You can't write there" },
  ]);
  assert.deepEqual(seen, [["a", 1, 3], ["b", 2, 3], ["c", 3, 3]]);
  assert.deepEqual(calls.map((c) => c.key), ["POST /api/projects/prj_me/docs/a/copies", "POST /api/projects/prj_me/docs/b/copies", "POST /api/projects/prj_me/docs/c/copies"], "in order");
  assert.deepEqual(calls[0].body, { to: "prj_team" });
  assert.ok(calls.every((c) => !c.key.startsWith("DELETE") && !c.key.startsWith("PUT")), "the original is left alone");
});

test("a Doc written on the platform goes the same way: made, then there, from one route and nothing else", async () => {
  let n = 0;
  const calls = platform({
    "POST /api/projects/prj_me/docs/p/copies": () => (n++ ? json(409, { error: "already", id: "t_p" }) : json(200, { doc: { id: "t_p" } })),
  });
  const first = await moveDocsToTeam("prj_me", "prj_team", [doc("p", "Plan")]);
  const again = await moveDocsToTeam("prj_me", "prj_team", [doc("p", "Plan")]);
  assert.deepEqual(first, [{ id: "p", title: "Plan", outcome: "made", teamDoc: "t_p" }]);
  assert.deepEqual(again, [{ id: "p", title: "Plan", outcome: "there", teamDoc: "t_p" }]);
  assert.deepEqual(calls.map((c) => c.key), Array(2).fill("POST /api/projects/prj_me/docs/p/copies"), "no read of the sections, no POST …/docs");
});

test("a Doc that can't be read, or a platform that can't be reached, refuses that Doc and the rest still go", async () => {
  platform({
    "POST /api/projects/prj_me/docs/gone/copies": json(404, { error: "no such Doc" }),
    "POST /api/projects/prj_me/docs/m/copies": () => {
      throw new Error("network down");
    },
    "POST /api/projects/prj_me/docs/ok/copies": json(200, { doc: { id: "t_ok" } }),
  });
  const out = await moveDocsToTeam("prj_me", "prj_team", [doc("gone", "Gone"), doc("m", "M", "d"), doc("ok", "Ok", "d")]);
  assert.deepEqual(out.map((r) => [r.id, r.outcome, r.why]), [["gone", "refused", "no such Doc"], ["m", "refused", "network down"], ["ok", "made", undefined]]);
});

test("a 409 with no team Doc named is a refusal, not a link to nothing", async () => {
  platform({ "POST /api/projects/prj_me/docs/a/copies": json(409, { error: "conflict" }) });
  const [r] = await moveDocsToTeam("prj_me", "prj_team", [doc("a", "A", "d")]);
  assert.deepEqual(r, { id: "a", title: "A", outcome: "refused", why: "conflict" });
});
