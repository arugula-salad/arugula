// Moving Docs into a team (Arugula #818, #863): what each Doc's outcome is,
// against a fake platform. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { moveDocsToTeam, resultWords } from "../src/shared/docs/move-docs.ts";

const doc = (id, title, machine = null, machine_name = null) => ({ id, title, machine, machine_name });
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

test("a Doc written on the platform moves in place; a 409 with its id is already there; a refusal says why", async () => {
  const calls = platform({
    "POST /api/projects/prj_me/docs/a/move": json(200, { doc: { id: "a" } }),
    "POST /api/projects/prj_me/docs/b/move": json(409, { error: "already in a team", id: "b" }),
    "POST /api/projects/prj_me/docs/c/move": json(403, { error: "You can't write there" }),
  });
  const seen = [];
  const out = await moveDocsToTeam("prj_me", "prj_team", [doc("a", "A"), doc("b", "B"), doc("c", "C")], (r, n, of) => seen.push([r.id, n, of]));
  assert.deepEqual(out, [
    { id: "a", title: "A", outcome: "made", teamDoc: "a" },
    { id: "b", title: "B", outcome: "there", teamDoc: "b" },
    { id: "c", title: "C", outcome: "refused", why: "You can't write there" },
  ]);
  assert.deepEqual(seen, [["a", 1, 3], ["b", 2, 3], ["c", 3, 3]]);
  assert.deepEqual(calls.map((c) => c.key), ["POST /api/projects/prj_me/docs/a/move", "POST /api/projects/prj_me/docs/b/move", "POST /api/projects/prj_me/docs/c/move"], "in order, one route each");
  assert.deepEqual(calls[0].body, { to: "prj_team" });
  assert.equal(resultWords(out[0], "Acme"), "Moved to Acme");
});

test("a machine's copy isn't moved from the platform: refused, naming its machine, and nothing is asked", async () => {
  const calls = platform({});
  const out = await moveDocsToTeam("prj_me", "prj_team", [doc("m", "M", "dev_1", "geek"), doc("n", "N", "dev_2")]);
  assert.deepEqual(out, [
    { id: "m", title: "M", outcome: "refused", why: "Move it from geek's Docs" },
    { id: "n", title: "N", outcome: "refused", why: "Move it from its machine's Docs" },
  ]);
  assert.equal(calls.length, 0);
});

test("a Doc that can't be read, or a platform that can't be reached, refuses that Doc and the rest still go", async () => {
  platform({
    "POST /api/projects/prj_me/docs/gone/move": json(404, { error: "no such Doc" }),
    "POST /api/projects/prj_me/docs/m/move": () => {
      throw new Error("network down");
    },
    "POST /api/projects/prj_me/docs/ok/move": json(200, { doc: { id: "ok" } }),
  });
  const out = await moveDocsToTeam("prj_me", "prj_team", [doc("gone", "Gone"), doc("m", "M"), doc("ok", "Ok")]);
  assert.deepEqual(out.map((r) => [r.id, r.outcome, r.why]), [["gone", "refused", "no such Doc"], ["m", "refused", "network down"], ["ok", "made", undefined]]);
});

test("a 409 with no Doc named is a refusal, not a link to nothing", async () => {
  platform({ "POST /api/projects/prj_me/docs/a/move": json(409, { error: "conflict" }) });
  const [r] = await moveDocsToTeam("prj_me", "prj_team", [doc("a", "A")]);
  assert.deepEqual(r, { id: "a", title: "A", outcome: "refused", why: "conflict" });
});
