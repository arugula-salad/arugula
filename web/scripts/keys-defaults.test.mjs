// The default keys (Arugula #1534): the controls (src/shared/controls.ts)
// may not clash, so a PR that adds a clashing default fails here. One verb
// per letter: two controls on one plain key do the same thing. One key per
// verb: Answer is the same key wherever it applies. Two in one place never
// share a chord, and a `g` place never shadows a plain `g`. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";

globalThis.window = new EventTarget();
const { CONTROLS } = await import("../src/shared/controls.ts");

const all = Object.entries(CONTROLS);
/** `*+key:escape` is Escape with any modifiers: it takes plain Escape too. */
const norm = (chord) => chord.replace(/^\*\+/, "");
const plain = (chord) => !/\b(Ctrl|Meta|Alt)\+/.test(chord);

/** Every pair of controls that share a chord on an OS. */
function shared() {
  const out = [];
  for (const os of ["mac", "other"]) {
    const by = new Map();
    for (const [id, c] of all)
      for (const k of new Set(c.keys[os].map(norm))) {
        for (const other of by.get(k) ?? []) out.push({ os, key: k, a: other, b: id });
        by.set(k, [...(by.get(k) ?? []), id]);
      }
  }
  return out;
}

test("a plain key means one verb, wherever it's bound", () => {
  const bad = shared()
    .filter(({ key, a, b }) => plain(key) && (!CONTROLS[a].verb || CONTROLS[a].verb !== CONTROLS[b].verb))
    .map(({ os, key, a, b }) => `${os} ${key}: ${a} and ${b}`);
  assert.deepEqual(bad, []);
});

test("two controls in one place never share a chord", () => {
  const bad = shared()
    .filter(({ a, b }) => CONTROLS[a].where === CONTROLS[b].where)
    .map(({ os, key, a, b }) => `${os} ${key}: ${a} and ${b} (${CONTROLS[a].where})`);
  assert.deepEqual(bad, []);
});

test("one verb has one key: its first chord is the same everywhere", () => {
  const first = new Map();
  const bad = [];
  for (const [id, c] of all) {
    if (!c.verb || !c.keys.mac.length) continue;
    const k = norm(c.keys.mac[0]);
    const was = first.get(c.verb);
    if (was && was.k !== k) bad.push(`${c.verb}: ${was.id} is ${was.k}, ${id} is ${k}`);
    else first.set(c.verb, { id, k });
  }
  assert.deepEqual(bad, []);
});

test("a sequence's first key is never a key of its own", () => {
  for (const os of ["mac", "other"]) {
    const chords = all.flatMap(([id, c]) => c.keys[os].map((k) => ({ id, k: norm(k) })));
    const firsts = new Set(chords.filter((x) => x.k.includes(" ")).map((x) => x.k.split(" ")[0]));
    assert.deepEqual(chords.filter((x) => firsts.has(x.k)).map((x) => `${os} ${x.k}: ${x.id}`), []);
  }
});

test("the core is Aaron's: j k ↵ esc l a / s ?, ⌘K and the seven g places", () => {
  const core = all.filter(([, c]) => c.core).map(([id]) => id).sort();
  assert.deepEqual(core, ["back", "go.agents", "go.boards", "go.chat", "go.docs", "go.inbox", "go.machines", "go.overview", "goto", "keys", "panel-focus", "panel-staff"]);
  // The core's page keys: each page binds them to the same verbs.
  const verbOf = (k) => new Set(all.filter(([, c]) => c.keys.mac.map(norm).includes(k)).map(([, c]) => c.verb));
  assert.deepEqual([...verbOf("key:j")], ["next"]);
  assert.deepEqual([...verbOf("key:k")], ["prev"]);
  assert.deepEqual([...verbOf("Enter")], ["open"]);
  assert.deepEqual([...verbOf("key:a")], ["answer"]);
  assert.deepEqual([...verbOf("key:/")], ["find"]);
  assert.deepEqual([...verbOf("key:s")], ["staff"]);
});

test("Look, light/dark and depth have no key by default", () => {
  for (const id of ["look", "look.theme", "look.depth"]) assert.deepEqual(CONTROLS[id].keys, { mac: [], other: [] }, id);
});
