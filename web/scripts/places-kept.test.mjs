// Where you were, per project (Arugula #989): which pages are a project's
// place, what's kept and when, and that a kept path is only ever a page of
// its own project. Run with `node --experimental-strip-types`.
import { test, mock } from "node:test";
import assert from "node:assert/strict";
import { PROJECT_KEY, SETTLE_MS, savePlaces, keptPath, lastProject, notePane, notePlace, paneKey, paneOf, placeIn, placeKey, placeOf } from "../src/platform/places-kept.ts";
import { parse } from "../src/platform/paths.ts";
import { pref, setSeenBackend } from "../src/shared/seen.ts";

const P = "prj_3e5d802cb4b08d43074df6c32f5344c1";
const Q = "prj_00000000000000000000000000000001";
const D = "da5b786733ac68a9";

test("a project's pages are its places; chat, settings, the inbox and the lists aren't", () => {
  for (const path of [`/p/${P}`, `/p/${P}/boards`, `/p/${P}/docs`, `/p/${P}/sessions`, `/p/${P}/runs`, `/p/${P}/machines/${D}?pane=3`, `/p/${P}/machines/swarm`, `/p/${P}/settings`])
    assert.equal(placeIn(parse(...path.split(/(?=\?)/))), P, path);
  for (const path of ["/", "/inbox", "/boards", "/settings", "/design", `/p/${P}/chat`, `/p/${P}/dm/dm_00aa11bb22cc33dd`, "/machines", "/nowhere"]) assert.equal(placeIn(parse(path)), null, path);
});

test("a kept path is only ever one of this origin's pages of its own project", () => {
  assert.equal(keptPath(`/p/${P}/docs`, P), `/p/${P}/docs`);
  assert.equal(keptPath(`/p/${P}-arugula/machines/${D}?pane=3`, P), `/p/${P}-arugula/machines/${D}?pane=3`);
  for (const v of [`/p/${Q}/docs`, "//evil.example/p/x", "https://evil.example/", "/inbox", 42, null, `/p/${P}/${"x".repeat(400)}`]) assert.equal(keptPath(v, P), null, String(v));
});

test("steps are kept once they settle, as the project and its place; leaving the page saves at once", () => {
  setSeenBackend(null);
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    notePlace(parse(`/p/${P}/boards`), `/p/${P}/boards`);
    notePlace(parse(`/p/${P}/docs`), `/p/${P}/docs`);
    assert.equal(pref(placeKey(P)), null, "not saved before it settles");
    assert.equal(placeOf(P), `/p/${P}/docs`, "but read as it is now");
    mock.timers.tick(SETTLE_MS);
    assert.equal(placeOf(P), `/p/${P}/docs`);
    assert.equal(lastProject(), P);
    // Chat isn't a place: the project and its place stay.
    notePlace(parse(`/p/${Q}/chat`), `/p/${Q}/chat`);
    mock.timers.tick(SETTLE_MS);
    assert.equal(lastProject(), P);
    assert.equal(placeOf(Q), null);
    // Leaving the page saves what waits.
    notePane(P, `/p/${P}/machines/${D}?pane=7`);
    savePlaces();
    assert.equal(paneOf(P), `/p/${P}/machines/${D}?pane=7`);
    // Another project's pane isn't this one's.
    notePane(P, `/p/${Q}/machines/${D}?pane=1`);
    mock.timers.tick(SETTLE_MS);
    assert.equal(pref(paneKey(P)), `/p/${P}/machines/${D}?pane=7`);
    assert.equal(pref(PROJECT_KEY), P);
  } finally {
    mock.timers.reset();
  }
});
