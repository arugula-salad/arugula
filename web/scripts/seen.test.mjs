// What a person has seen (shared/seen.ts): per browser in localStorage,
// per account through control's `/api/me/prefs`; nothing counts as unseen
// before it has loaded; only what changed is written, so another tab's
// writes stay; Tips are on until turned off. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { accountBackend, bigTipsOn, FACTS_KEPT, loadSeen, localBackend, markSeen, markSeenFact, pref, seen, seenFact, seenReady, setBigTips, setPref, setSeenBackend, setTips, tipsOn } from "../src/shared/seen.ts";

/** A Storage of our own. */
function storage(init = {}) {
  const m = new Map(Object.entries(init));
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, String(v)), m };
}
const stored = (s) => JSON.parse(s.getItem("arugula.seen") ?? "{}");
const settle = () => new Promise((r) => setTimeout(r, 0));

test("before a backend has loaded, everything counts as seen", async () => {
  setSeenBackend(null);
  assert.equal(seenReady(), false);
  assert.equal(seen("port-listening"), true);
  let answer;
  setSeenBackend(accountBackend(() => new Promise((r) => (answer = r)), async () => {}));
  assert.equal(seen("port-listening"), true, "still loading");
  answer({});
  await loadSeen();
  assert.equal(seenReady(), true);
  assert.equal(seen("port-listening"), false);
});

test("this browser's: kept in localStorage, and another tab's writes stay", async () => {
  const s = storage();
  setSeenBackend(localBackend(s));
  await loadSeen();
  assert.equal(seen("port-listening"), false);
  markSeen("port-listening", 17);
  await settle();
  assert.deepEqual(stored(s), { "seen:port-listening": 17 });
  // Another tab writes something else; this one's next write keeps it.
  s.setItem("arugula.seen", JSON.stringify({ ...stored(s), "seen:pr-link": 3 }));
  setPref("whats-new", "0.64.0");
  await settle();
  assert.deepEqual(stored(s), { "seen:port-listening": 17, "seen:pr-link": 3, "whats-new": "0.64.0" });
  // A new page reads it all back.
  setSeenBackend(localBackend(s));
  await loadSeen();
  assert.equal(seen("port-listening"), true);
  assert.equal(seen("pr-link"), true);
  assert.equal(pref("whats-new"), "0.64.0");
  // Junk in the entry is nothing seen, not an error.
  setSeenBackend(localBackend(storage({ "arugula.seen": "{not json" })));
  await loadSeen();
  assert.equal(seen("port-listening"), false);
});

test("the account's: loads once, and writes only what changed", async () => {
  const server = { "seen:pr-link": 1 };
  const writes = [];
  let loads = 0;
  const backend = accountBackend(
    async () => (loads++, { ...server }),
    async (c) => {
      writes.push(c);
      for (const [k, v] of Object.entries(c)) v === null ? delete server[k] : (server[k] = v);
    },
  );
  setSeenBackend(backend);
  await Promise.all([loadSeen(), loadSeen()]);
  assert.equal(loads, 1);
  assert.equal(seen("pr-link"), true);
  assert.equal(seen("port-listening"), false);
  markSeen("port-listening", 5);
  assert.equal(seen("port-listening"), true, "at once, before it's saved");
  await settle();
  assert.deepEqual(writes, [{ "seen:port-listening": 5 }]);
  // Another browser signed in to the account sees it.
  setSeenBackend(accountBackend(async () => ({ ...server }), async () => {}));
  await loadSeen();
  assert.equal(seen("port-listening"), true);
});

test("a control that can't answer gives way to this browser's", async () => {
  setSeenBackend(accountBackend(() => Promise.reject(new Error("404")), async () => {}));
  await loadSeen();
  await settle();
  assert.equal(seenReady(), true);
  assert.equal(seen("port-listening"), false);
});

test("Tips are on until turned off, and on again takes the pref back", async () => {
  const s = storage();
  setSeenBackend(localBackend(s));
  await loadSeen();
  assert.equal(tipsOn(), true);
  setTips(false);
  assert.equal(tipsOn(), false);
  await settle();
  assert.deepEqual(stored(s), { tips: false });
  setTips(true);
  assert.equal(tipsOn(), true);
  await settle();
  assert.deepEqual(stored(s), {});
});

test("Big tips are on until a pane modal's opt-out, apart from Tips", async () => {
  const s = storage();
  setSeenBackend(localBackend(s));
  await loadSeen();
  assert.equal(bigTipsOn(), true);
  setBigTips(false);
  assert.equal(bigTipsOn(), false);
  assert.equal(tipsOn(), true, "chips and toasts carry on");
  await settle();
  assert.deepEqual(stored(s), { "tips.modal": "off" });
  setBigTips(true);
  assert.equal(bigTipsOn(), true);
  await settle();
  assert.deepEqual(stored(s), {});
});

test("a repeating hint is seen per fact, keeping its last few", async () => {
  setSeenBackend(null);
  assert.equal(seenFact("nvim-key-tip", "3:100"), true, "not loaded yet");
  const s = storage();
  setSeenBackend(localBackend(s));
  await loadSeen();
  assert.equal(seenFact("nvim-key-tip", "3:100"), false);
  markSeenFact("nvim-key-tip", "3:100");
  assert.equal(seenFact("nvim-key-tip", "3:100"), true);
  assert.equal(seenFact("nvim-key-tip", "3:101"), false, "a new nvim is a new fact");
  for (let i = 0; i < FACTS_KEPT; i++) markSeenFact("nvim-key-tip", `4:${i}`);
  assert.equal(seenFact("nvim-key-tip", "3:100"), false, "the oldest goes");
  assert.equal(seenFact("nvim-key-tip", `4:${FACTS_KEPT - 1}`), true);
  await settle();
  const kept = stored(s)["seen:nvim-key-tip"];
  assert.equal(kept.length, FACTS_KEPT);
  assert.ok(JSON.stringify(kept).length <= 512, "fits a pref");
});
