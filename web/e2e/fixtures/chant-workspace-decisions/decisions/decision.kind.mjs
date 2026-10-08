// The decision record kind (#2555), data only: no imports, no code.
//
// chant workspace records --kind decisions/decision.kind.mjs --current --json
//
// A copy of docs/design/decisions/decision.kind.mjs in the chant repo, with
// decision.schema.json beside it, so a workspace made from this one reads its
// decisions without the chant repo. chant's test/reference-workspace.test.ts fails
// when this kind's data or the schema drifts from chant's.
export const recordKind = {
  name: "decision",
  location: { dir: ".", match: "^[a-z][a-z0-9]{0,15}-[0-9]{3,}-.+\\.md$" },
  format: "markdown-front-matter",
  schema: { id: "urn:intentius:chant:decision:1", path: "decision.schema.json" },
  idField: "id",
  stateField: "state",
  states: ["proposed", "decided", "ratified", "superseded", "withdrawn"],
  // Sealed once reached (#2555): records amend and new write the whole-file
  // seal into closed_digest when a decision enters one, and records checks it
  // on every read (#2546, ws-063).
  closedStates: ["ratified", "superseded"],
  seal: { field: "closed_digest" },
  // The current decisions, with the files they pin, are the workspace's spec:
  // records --current --json lists them under spec (#2524 D20, #2546).
  spec: true,
  supersedes: { field: "supersedes", key: "decision" },
  // A remediates link names a closed decision this one fixes the consequence
  // of, without replacing it: the target's state never changes (#2774).
  remediates: { field: "remediates", key: "decision" },
  // A supersedes link takes effect under an equal or stricter approval rule
  // (#2524 D4): from a record ranked above 0 and at least as high as the one it
  // names. A decided record supersedes a decided or proposed one, a ratified
  // record supersedes any, and a proposed or withdrawn record none.
  approval: { proposed: 0, withdrawn: 0, decided: 1, ratified: 2, superseded: 2 },
  // Evidence entries with a path pin a workspace file by the hash of its bytes (#2549).
  pins: { field: "evidence" },
  // member:<name> and path:<path> entries are the record's links in workspace graph (#2549).
  constrains: { field: "constrains" },
  // Workspace paths a change carrying the decision out must not touch
  // (#2773): chant workspace check --changes reports change-out-of-scope.
  outOfScope: { field: "out_of_scope" },
  // Verdicts, and the field naming the decider, for each record's digest and
  // quorum (#2671, #2672). A decision becomes ratified only once its quorum
  // is met: records new and amend refuse it below, and the digest leaves the
  // state out, so ratifying keeps the verdicts counting (#2873).
  reviews: { field: "reviews", decider: "decided_by", ratified: "ratified" },
  // records new --by and the MCP records-new tool's by name a proposal's
  // proposer here, apart from decided_by, which stays null until the
  // decision is decided (#2756).
  proposedBy: { field: "proposed_by" },
  // source is where a decision came from, and says where its proposal came
  // from too: via, client, harness, model, session, turns and a transcript
  // pinned by hash (#2708).
  source: { field: "source" },
};
