// The platform page's tab titles (UI audit #159): the page, its project,
// then Arugula; a name not yet known is left out. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { parse } from "../src/platform/paths.ts";
import { titleOf } from "../src/platform/titles.ts";
import { shellTitle } from "../src/shell/ui/titles.ts";

const known = {
  project: (id) => ({ prj_aaaa: "Acme" })[id],
  issue: (id) => (id === "iss_bbbb" ? { number: 142, title: "M265 the bar" } : undefined),
  run: (id) => (id === "run_cccc" ? "ci.test" : undefined),
  room: (id) => ({ ch_dddd: "#general", dm_eeee: "sam" })[id],
};
const t = (path, k = known) => {
  const [p, q = ""] = path.split("?");
  return titleOf(parse(p, q ? `?${q}` : ""), k);
};

test("each route has a title of its own", () => {
  assert.equal(t("/"), "Projects · Arugula");
  assert.equal(t("/p/prj_aaaa-acme"), "Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/runs"), "Runs · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/runs/run_cccc"), "ci.test · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/issues"), "Issues · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/issues/iss_bbbb-m265"), "#142 M265 the bar · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/issues/new"), "New issue · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/settings"), "Settings · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/deploys/new"), "Deploy… · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/workspaces"), "Cloud workspaces · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/chat/ch_dddd-general"), "#general · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/chat/activity"), "Activity · Chat · Acme · Arugula");
  assert.equal(t("/inbox"), "Inbox · Arugula");
  assert.equal(t("/settings"), "Account settings · Arugula");
  assert.equal(t("/nowhere/at/all"), "Not found · Arugula");
});

test("a DM is in its project, a deploy takes the project the bar shows; unknown names are left out", () => {
  assert.equal(t("/p/prj_aaaa/dm/dm_eeee"), "sam · Acme · Arugula");
  assert.equal(t("/p/prj_aaaa/dm/dm_ffff"), "Direct message · Acme · Arugula");
  const dep = "/deploys/dep_" + "0".repeat(32);
  assert.equal(t(dep), "Deploy · Arugula");
  assert.equal(t(dep, { ...known, shown: "prj_aaaa" }), "Deploy · Acme · Arugula");
  assert.equal(t("/p/prj_ffff/issues/iss_ffff"), "Issue · Arugula");
});

// Control's tabs read the same way (UI audit #170).
test("control's tabs say the session and the machine, or the place", () => {
  assert.equal(shellTitle("panes", "geek", "api"), "api · geek · Arugula");
  assert.equal(shellTitle("panes", "geek", null), "geek · Arugula");
  assert.equal(shellTitle("panes", null, null), "Arugula");
  assert.equal(shellTitle("swarm", "geek", "api"), "Swarm · Arugula");
  assert.equal(shellTitle("overview", "geek", "api"), "Sessions · geek · Arugula");
  assert.equal(shellTitle("agents", "geek", "api"), "Agents · geek · Arugula");
});
