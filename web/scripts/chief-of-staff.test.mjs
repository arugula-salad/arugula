// A chief of staff's session is named by its project's id (Arugula #1457):
// people read it by the project's name where the page knows the project,
// else as it is; every other session's name shows as it is. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { chiefProject, sessionLabel } from "../src/shared/chief-of-staff.ts";

const names = { prj_3e5d80aa: "Launch crew", prj_77: "Other crew", prj_blank: "  " };
const projectName = (id) => names[id];

test("a chief of staff's session reads by its project's name, and falls back to its own", () => {
  assert.equal(chiefProject("Chief of staff · prj_3e5d80aa"), "prj_3e5d80aa");
  assert.equal(sessionLabel("Chief of staff · prj_3e5d80aa", projectName), "Chief of staff · Launch crew");
  // Two projects of one name each read by it; their names still differ.
  assert.equal(sessionLabel("Chief of staff · prj_77", projectName), "Chief of staff · Other crew");
  // A project the page hasn't loaded, one with no name, or no lookup: as it is.
  assert.equal(sessionLabel("Chief of staff · prj_unknown", projectName), "Chief of staff · prj_unknown");
  assert.equal(sessionLabel("Chief of staff · prj_blank", projectName), "Chief of staff · prj_blank");
  assert.equal(sessionLabel("Chief of staff · prj_3e5d80aa"), "Chief of staff · prj_3e5d80aa");
});

test("no other session's name changes", () => {
  for (const other of ["Chief of staff", "Chief of staff · Launch crew", "My staff", "fix-1457", "Chief of staff · prj_3e5d80aa · extra", "chief of staff · prj_3e5d80aa"]) {
    assert.equal(chiefProject(other), null, other);
    assert.equal(sessionLabel(other, projectName), other);
  }
});
