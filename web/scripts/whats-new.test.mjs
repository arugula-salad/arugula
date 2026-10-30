// What's new (shared/whats-new.ts): the releases after the last one shown,
// up to this build; the newest few on a first run; never moved back by an
// older build; items this person can't use left out. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { advances, compareVersions, MAX_RELEASES, unshown, upTo, usable } from "../src/shared/whats-new.ts";
import { releases } from "../../scripts/whats-new.mjs";

const rel = (version, ...items) => ({ version, items: items.map((id) => (typeof id === "string" ? { id, title: id, line: id } : id)) });
const list = [rel("0.65.0", "e"), rel("0.64.1", "d"), rel("0.64.0", "c"), rel("0.63.9", "b"), rel("0.63.8", "a")];
const versions = (rs) => rs.map((r) => r.version);

test("versions compare by number, not as text", () => {
  assert.ok(compareVersions("0.63.10", "0.63.9") > 0);
  assert.ok(compareVersions("1.0.0", "0.99.99") > 0);
  assert.equal(compareVersions("0.64.0", "0.64.0"), 0);
  assert.ok(compareVersions("0.1.0", null) > 0, "anything is newer than nothing");
  assert.ok(compareVersions("junk", "0.1.0") < 0);
});

test("after an update: the releases since the last shown, up to this build", () => {
  assert.deepEqual(versions(unshown(list, "0.64.1", "0.63.9")), ["0.64.1", "0.64.0"]);
  assert.deepEqual(versions(unshown(list, "0.64.1", "0.64.1")), [], "already shown");
  assert.deepEqual(versions(unshown(list, "0.64.0", "0.64.1")), [], "an older build shows nothing");
});

test("a first run, or a long gap, shows the newest few", () => {
  const shown = (built, last) => versions(usable(unshown(list, built, last), () => true));
  assert.deepEqual(shown("0.65.0", null), ["0.65.0", "0.64.1", "0.64.0"]);
  assert.deepEqual(shown("0.65.0", "0.1.0").length, MAX_RELEASES);
  assert.deepEqual(shown("0.65.0", { not: "a version" }), ["0.65.0", "0.64.1", "0.64.0"]);
});

test("releases with nothing this person can use don't take the newest few's places", () => {
  // As on a daemon's own page after releases that were all control's.
  const team = (v) => rel(v, { id: v, title: v, line: v, needs: ["team"] });
  const rs = [team("0.66.2"), team("0.66.1"), team("0.66.0"), ...list];
  assert.deepEqual(versions(usable(unshown(rs, "0.66.2", "0.1.0"), (n) => n === "control")), ["0.65.0", "0.64.1", "0.64.0"]);
});

test("notes for a release not built yet stay out", () => {
  // Notes land on main before the bump: a build of 0.64.0 has 0.64.1's.
  assert.deepEqual(versions(upTo(list, "0.64.0")), ["0.64.0", "0.63.9", "0.63.8"]);
  assert.deepEqual(upTo(list, null), [], "outside a build");
});

test("the pref moves on, never back", () => {
  assert.equal(advances("0.64.1", "0.64.0"), true);
  assert.equal(advances("0.64.1", null), true);
  assert.equal(advances("0.64.1", "0.64.1"), false);
  assert.equal(advances("0.64.0", "0.64.1"), false);
  assert.equal(advances(null, null), false);
});

test("items this person can't use are left out, and releases left empty", () => {
  const rs = [
    rel("0.64.0", { id: "x", title: "x", line: "x", needs: ["team"] }, "y"),
    rel("0.63.9", { id: "z", title: "z", line: "z", needs: ["hosted"] }),
    rel("0.63.8", { id: "w", title: "w", line: "w", needs: ["control", "flag:boards"] }),
  ];
  const got = usable(rs, (n) => n === "control");
  assert.deepEqual(got.map((r) => [r.version, r.items.map((i) => i.id)]), [["0.64.0", ["y"]]]);
  const all = usable(rs, () => true);
  assert.equal(all.length, 3);
});

test("the release notes' headlines: newest first, one id each", () => {
  const RELEASES = releases();
  assert.ok(RELEASES.length >= 3, "the 0.63.x backfill");
  for (let i = 1; i < RELEASES.length; i++) assert.ok(compareVersions(RELEASES[i - 1].version, RELEASES[i].version) > 0);
  const ids = RELEASES.flatMap((r) => r.items.map((i) => i.id));
  assert.equal(new Set(ids).size, ids.length);
});
