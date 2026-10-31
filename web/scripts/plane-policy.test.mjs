// What agents may write to a Plane project (Arugula #423): which preset a
// stored policy is, and "Custom" when it is none. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { PRESETS, presetOf, attributionHelp } from "../src/platform/settings/plane-policy.ts";

test("a stored policy is the preset it equals, or Custom", () => {
  assert.deepEqual(PRESETS.map((p) => presetOf(p.policy)), PRESETS.map((p) => p.name));
  const middle = PRESETS[1].policy;
  assert.equal(presetOf({ ...middle, fields: "approve" }), "Custom");
  assert.equal(presetOf({ create: "allow", comment: "approve", edit: "allow", state: "approve", fields: "allow" }), "Custom");
});

test("the attribution help shows the exact line each sends", () => {
  assert.match(attributionHelp("short"), /: — via Arugula$/);
  assert.match(attributionHelp("full"), /— via Arugula, by Ada's agent on geek$/);
  assert.match(attributionHelp("none"), /no line added/);
});
