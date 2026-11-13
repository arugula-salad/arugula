// Fixture data for the "Move to project…" spike (Arugula #971): Aaron's
// Personal project as it was on 2026-11-13, and the same at ten times the
// size (about what Jake has), with nothing fetched. The screen reads only
// this file's types and `./plan`; when the planner (#968) lands, its answer
// replaces `SOURCES` and `TARGETS`, and nothing else here moves.

/** A project the work moves from, or into. */
export interface Project {
  id: string;
  name: string;
  /** Whose it is: "Personal", or a team's name. */
  owner: string;
  /** The number its next native issue gets. */
  next: number;
}

/** An owner a new project can have. */
export interface Owner {
  key: string;
  name: string;
  /** Who sees what's in it, for "Before you move". */
  people: string;
}

export interface Repo {
  id: string;
  /** `owner/name`. */
  name: string;
  /** Its GitHub issues and PRs mirrored here: they follow the repo. */
  github: number;
}

export interface Board {
  id: string;
  name: string;
  /** The repo it's tied to; none, and it moves on its own. */
  repo?: string;
  rows: number;
}

export interface Issue {
  id: string;
  n: number;
  title: string;
  closed?: boolean;
  /** The repo it closes in. */
  repo?: string;
  /** The board it's on, when it has no repo. */
  board?: string;
}

export interface Doc {
  id: string;
  title: string;
  /** The machine whose library has it; none, and it's on the platform. */
  machine?: string;
  /** The board it's linked to (its Docs strip, or a row). */
  board?: string;
}

/** Everything a project holds that can move. */
export interface Source {
  key: string;
  label: string;
  project: Project;
  repos: Repo[];
  boards: Board[];
  issues: Issue[];
  docs: Doc[];
}

/** This Mac: where the screen is open, in the spike (see `here` in the page). */
export const THIS_MAC = "aarons-mac-mini";

export const OWNERS: Owner[] = [
  { key: "personal", name: "Personal", people: "you alone" },
  { key: "tm_arugula", name: "Arugula", people: "everyone in the Arugula team" },
];

/** Existing projects the reader can write to, other than the source. */
export const TARGETS: Project[] = [{ id: "prj_arugula", name: "Arugula", owner: "Arugula", next: 975 }];

const repo = (name: string, github = 0): Repo => ({ id: `repo_${name.split("/")[1]}`, name, github });

const REPOS: Repo[] = [
  repo("alecraso/bradbury", 19),
  repo("alecraso/factorio-companion-bridge"),
  repo("alecraso/factorio-companion-mod"),
  repo("alecraso/factorio-mcp-server"),
  repo("alecraso/freight-gantries"),
  repo("alecraso/mogrify"),
  repo("alecraso/rhythm"),
];

const BOARDS: Board[] = [
  { id: "brd_bradbury", name: "Bradbury → playtest-b ships to main", repo: "repo_bradbury", rows: 19 },
  { id: "brd_rhythm", name: "Rhythm → dogfood on Android + iPhone", repo: "repo_rhythm", rows: 23 },
];

// [number, title, closed]
const RHYTHM: [number, string, boolean][] = [
  [3, "Rhythm (epic): routine-shaped calendar — dogfood on Android (Aaron) + iPhone (Bek)", false],
  [4, "Rhythm R1: tech research — cross-platform delivery + calendar sync", true],
  [5, "Rhythm R2: prior art + competitors", true],
  [6, "Rhythm: ADR — stack, platform, hosting", true],
  [7, "Rhythm UX: onboarding \"paint your rhythm\" prototype", true],
  [8, "Rhythm: scaffold Expo app + calendar probe screen", true],
  [9, "Rhythm: calendar adapter — on-device calendars, per-calendar tagging, event inference", true],
  [10, "Rhythm: day engine — materialize days, day-type effects, fit, load, sameness", true],
  [11, "Rhythm: rhythm editor v0 — blocks, variants, protected, weekday/weekend", true],
  [12, "Rhythm: Day view — blocks + events, day-type toggles, load, same/different", true],
  [13, "Rhythm: Week look-ahead (Sunday ritual)", true],
  [14, "Rhythm: heads-ups — transitions, leave-by, morning brief (push)", true],
  [15, "Rhythm: dogfood launch — Aaron on Android, Bek on iPhone", false],
  [16, "Rhythm: \"Today changed\" mid-day replan", false],
  [17, "Rhythm: evening check-in + load tuning", false],
  [18, "Rhythm: boundaries + new-invite intake", false],
  [19, "Rhythm: distribution — Apple Developer, EAS, APK to Aaron, TestFlight to Bek", false],
  [20, "Rhythm: app foundation — engine linked, Expo Router, local store, app tests", true],
  [21, "Rhythm: stable variant ids (renaming a variant breaks links)", true],
  [22, "Telemetry: OTLP logs + traces to a collector on the tailnet", true],
  [23, "Privacy copy: \"nothing leaves your phone\" is untrue on telemetry builds", false],
  [24, "Rhythm editor: drag blocks on a week grid, with kind colors", true],
  [25, "Fewer starter kinds, free-form labels instead of fixed variants", true],
  [26, "Your own day types; drop Kids home + WFH from the starter set", false],
  [27, "Drag block edges to resize within a day", false],
];

const ISSUES: Issue[] = [
  { id: "iss_1", n: 1, title: "Agents in agent blocks never see the doc tool's prototype guidance: tool descriptions are cut at 2048 characters", closed: true },
  { id: "iss_2", n: 2, title: "Docs list: share Docs without opening them, and move several of Mine into a team at once", closed: true },
  ...RHYTHM.map(([n, title, closed]): Issue => ({ id: `iss_${n}`, n, title, closed, repo: "repo_rhythm" })),
];

// The 60 Arugula Docs in this Mac's library. The first 34 had a platform
// copy too; after #865 there are no copies, so all 60 move from the Mac.
const MAC_DOCS = [
  "Sequencing on boards: a Gantt, or something else",
  "#731 Buttons in three sizes",
  "Chips: five shapes (#730)",
  "Spike #702: panes go anywhere",
  "Headers: the bar and the page header (#686)",
  "Design system inventory: 7 × 7 (#685)",
  "Design system: inventory (#685)",
  "Triage under Boards",
  "Agents in chat: name and reactions",
  "Wait and watch tools: research (#628)",
  "Wait and watch tools: review (#628)",
  "Spring card flip tuner",
  "#418 plan: one Docs list",
  "Spring card flip tuner (older)",
  "Agent-to-agent chat: review of the brief",
  "Agent-to-agent chat: brief for the next lead",
  "Boards from the agent side: feedback",
  "Arugula for product managers",
  "Launch plan: coming out of stealth",
  "HTML embeds in Docs: plan",
  "Plane epics as boards (#371)",
  "Boards in Arugula: the work, keeping itself up to date",
  "Item blocks: one design language",
  "Work management: what to learn from the field",
  "Boards, made opinionated",
  "One rail everywhere",
  "Boards, issues and agents: how it works now",
  "Issues on boards, phase 3: Plane",
  "Agent palettes for Docs",
  "Boards and Issues",
  "Boards manage issues, wherever they live",
  "AR-5: a live status board",
  "Shared Docs: what shipped in 0.51.0",
  "How the Doc block shipped",
  "#929 · The board's header",
  "CI and merge velocity",
  "Layouts that stay on",
  "Board flags without the card",
  "RFC: Projects as the main boundary",
  "Not finished (#841)",
  "One Docs view (#832)",
  "Boards: where the chart's controls live",
  "Starred tabs and panes (#854)",
  "Tickets from anywhere",
  "Onboarding: download to team",
  "Pricing proposal",
  "Onboarding: one level at a time",
  "Pricing: where the ladder pays",
  "Docs list: share and move (#814)",
  "Less text: what to cut",
  "Docs with tabs (#784)",
  "Icons: one set (#691)",
  "Cards: one card (#698)",
  "Share with the team",
  "Menus: one menu (#734)",
  "Rows: one selected state (#732)",
  "Type scale (#736)",
  "Dialogs, one side sheet (#733)",
  "One empty state (#735)",
  "Colour (#688)",
];

const DOCS: Doc[] = MAC_DOCS.map((title, i) => ({ id: `doc_mac_${i}`, title, machine: THIS_MAC }));

const PERSONAL: Project = { id: "prj_personal", name: "Personal", owner: "Personal", next: 28 };

const aaron: Source = { key: "aaron", label: "Aaron's Personal", project: PERSONAL, repos: REPOS, boards: BOARDS, issues: ISSUES, docs: DOCS };

/** The same shape at `times` the size: more repos, boards tied to them and
 * some that aren't, issues on repos, on boards and loose, and platform Docs
 * linked to boards or not, beside this Mac's. Made up, and the same every
 * time. */
function scaled(times: number): Source {
  const words = ["atlas", "beacon", "cinder", "delta", "ember", "fjord", "grove", "harbor", "iris", "juniper"];
  const repos: Repo[] = [];
  const boards: Board[] = [];
  const issues: Issue[] = [];
  const docs: Doc[] = [];
  let n = 1;
  for (let k = 0; k < times; k++) {
    const w = words[k % words.length];
    for (const r of REPOS) {
      const name = `${r.name}-${w}`;
      repos.push({ id: `repo_${name.split("/")[1]}`, name, github: r.github + ((k * 7) % 13) });
    }
    const tied = repos[repos.length - 7];
    boards.push({ id: `brd_${w}_ship`, name: `${w[0].toUpperCase()}${w.slice(1)} → ship it`, repo: tied.id, rows: 15 + k });
    boards.push({ id: `brd_${w}_plan`, name: `${w[0].toUpperCase()}${w.slice(1)} planning`, rows: 8 + k });
    for (let i = 0; i < 30; i++) {
      const on: Partial<Issue> = i < 20 ? { repo: tied.id } : i < 27 ? { board: `brd_${w}_plan` } : {};
      issues.push({ id: `iss_${n}`, n, title: `${w}: ${["fix", "plan", "spike", "ship", "polish"][i % 5]} ${i + 1}`, closed: i % 3 === 0, ...on });
      n++;
    }
    for (let i = 0; i < 6; i++) docs.push({ id: `doc_${w}_${i}`, title: `${w}: notes ${i + 1}`, board: i < 3 ? `brd_${w}_ship` : i < 5 ? `brd_${w}_plan` : undefined });
  }
  for (let k = 0; k < times; k++) for (const [i, d] of DOCS.entries()) docs.push({ id: `${d.id}_${k}`, title: k ? `${d.title} (${k + 1})` : d.title, machine: i % 2 ? THIS_MAC : "jakes-laptop" });
  return { key: "jake", label: "10× (Jake's size)", project: { ...PERSONAL, next: n }, repos, boards, issues, docs };
}

export const SOURCES: Source[] = [aaron, scaled(10)];
