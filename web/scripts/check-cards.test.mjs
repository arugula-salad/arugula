import assert from "node:assert/strict";
import { test } from "node:test";
import { ALLOWED, offenders, swarmOffenders } from "./check-cards.mjs";

const bad = (css, file = "shared/x.css") => offenders(css, file).map((o) => [o.line, o.rule]);

test("a rule for a new -card class outside card.css fails", () => {
  assert.deepEqual(bad(".thing-card { padding: 4px; }"), [[1, "card"]]);
  assert.deepEqual(bad(".a { color: red; }\n\n.platform .thing-card:hover,\n.other { color: red; }"), [[3, "card"]]);
  assert.deepEqual(bad("@media (max-width: 600px) {\n  .thing-card { margin: 0; }\n}"), [[2, "card"]]);
});

test("card.css may define -card classes", () => {
  assert.deepEqual(bad(".thing-card { padding: 4px; }", "shared/ui/card.css"), []);
});

test("an allowed -card class, and classes that only contain the word, are fine", () => {
  assert.deepEqual(bad(".session-card { max-width: 560px; }"), []);
  assert.deepEqual(bad(".cardinal { color: red; }\n.card-title { color: red; }\n.cards-list { color: red; }"), []);
  assert.ok(ALLOWED.has("session-card") && !ALLOWED.has("sw-card"));
});

test("a literal border-radius on a .card fails, a token or a mark doesn't", () => {
  assert.deepEqual(bad(".card { border-radius: 8px; }"), [[1, "radius"]]);
  assert.deepEqual(bad(".card-dense {\n  padding: 0;\n  border-radius: 6px;\n}"), [[3, "radius"]]);
  assert.deepEqual(bad(".x .card:hover { border-radius: 4px 4px 0 0; }"), [[1, "radius"]]);
  assert.deepEqual(bad(".card { border-radius: var(--card-radius); }"), []);
  assert.deepEqual(bad(".card { border-radius: var(--card-radius) var(--card-radius) 0 0; }"), []);
  assert.deepEqual(bad(".card { border-radius: var(--card-radius, 8px); }"), []);
  assert.deepEqual(bad(".card { border-radius: 0; }"), []);
  assert.deepEqual(bad(".card-tone::before { border-radius: 0 3px 3px 0; }"), []);
  assert.deepEqual(bad(".button { border-radius: 8px; }"), []);
});

test("a literal radius fails in card.css too", () => {
  assert.deepEqual(bad(".card { border-radius: 8px; }", "shared/ui/card.css"), [[1, "radius"]]);
});

test("a .sw-card or .sw-cards fails, in a stylesheet and in a component", () => {
  assert.deepEqual(bad(".platform .sw-card { padding: 0; }"), [[1, "swarm"]]);
  assert.deepEqual(bad(".sw-cards > li { padding: 0; }"), [[1, "swarm"]]);
  assert.deepEqual(swarmOffenders('<li class={`sw-card ${t}`}>', "platform/x.tsx").map((o) => o.line), [1]);
  assert.deepEqual(swarmOffenders('<ul class="a sw-cards">', "platform/x.tsx").map((o) => o.line), [1]);
  assert.deepEqual(swarmOffenders("// the sw-card it replaced\nconst a = 1;", "platform/x.tsx"), []);
  assert.deepEqual(swarmOffenders('<li class="sw-cardio">', "platform/x.tsx"), []);
});

test("comments are ignored", () => {
  assert.deepEqual(bad("/* .thing-card { border-radius: 8px } */\n.a { color: red; }"), []);
});
