// The work item record kind (#2683), data only: no imports, no code.
//
// chant workspace records --kind work/work.kind.mjs --json
// chant workspace graph --intent <region> --kind decisions/decision.kind.mjs --kind work/work.kind.mjs
//
// A work item is a record in the workspace, with its dependencies written
// inside it, that people and agents share as one queue with no server. Most
// come from a gap the intent graph reports, named in `source.finding`.
// Records are Markdown with front matter: an item keeps its id while its
// state changes, so it is not content-addressed. ws-053 (#2664) lets a kind
// read `format: "json"`, but `records new` and `records amend` write Markdown
// only, so work items stay Markdown until the write commands write JSON.
export const recordKind = {
  name: "work",
  location: { dir: ".", match: "^W-[0-9]{3,}-.+\\.md$" },
  format: "markdown-front-matter",
  schema: { id: "urn:intentius:chant:work:1", path: "work.schema.json" },
  idField: "id",
  stateField: "state",
  // A proposal opens proposed: an item an agent or a decision point's model
  // suggested (ws-058, #2741). A person keeps it by moving it to open, or drops it.
  states: ["proposed", "open", "in-progress", "done", "dropped"],
  closedStates: ["done", "dropped"],
  // Takes effect from a done or dropped item, the closed states.
  supersedes: { field: "supersedes", key: "work" },
  // records new --by and the MCP records-new tool's by name a proposal's
  // proposer here (#2756).
  proposedBy: { field: "proposed_by" },
  // Evidence is the proof of done: links, or workspace files pinned by hash.
  pins: { field: "evidence" },
  // The same grammar as a decision's: member:, path:, issues and decision ids.
  constrains: { field: "constrains" },
  // Workspace paths the change doing the work must not touch (#2773):
  // chant workspace check --changes --work <id> reports change-out-of-scope.
  outOfScope: { field: "out_of_scope" },
  // needs and implements, the decisions they name, and when an item is ready.
  work: {
    needs: "needs",
    implements: "implements",
    decisions: "../decisions/decision.kind.mjs",
    open: "open",
    done: "done",
    closedOn: "closed_on",
    // What done means, stated on the record (#2772): each criterion expects
    // a verification, and a manual verdict never comes from the owner.
    acceptance: { field: "acceptance", implementer: "owner" },
    // The builder tiers an item may name in tier, declared once here (#3147).
    // The same three the slice-tier decision point answers with.
    tier: { field: "tier", tiers: ["small", "medium", "large"] },
    // An item is left to people after this many failed attempts, counted
    // from its lease history (chant workspace work history). An item's own
    // max_attempts overrides it (#3147).
    attempts: { field: "max_attempts", max: 3 },
    // Decision points' answers about an item are joined from the answer
    // kind by the item's id, never copied onto the item (#3147).
    answers: "../answers/answer.kind.mjs",
    // A workspace with a contract kind links items to contracts with
    // contract: { field: "contract", kind: "<contract kind file>" }.
  },
};
