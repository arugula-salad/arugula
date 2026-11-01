// `just demo-fleet` (#399): a seeded, believable fleet to screenshot, demo
// and load, on `just dev-control`'s throwaway control (dev-control.ts
// --demo). People with ordinary first names, each on a laptop of their own,
// and a team build box; terminals running stand-in builds, tests, servers
// and editors (e2e/stand-ins.ts, the swarm tests' fakes) at a neutral
// prompt; agent blocks in every state an agent is in (working, asking to
// run something, asking a question, failed, done), each with the context
// strip (#397) at its own fill, one compacted at low fill and one near
// full, and a task list (#445) with steps in every column of the swarm's
// plan board, one waiting on a blocker and one with no list; and in control, chat (#general and a DM), a board with rows in
// each lane, a linked repo's open PR, and a Doc.
//
// The agents are the scripted fake ACP agent (crates/daemon/tests/
// fake_acp.py --demo FILE) started as Claude Code: each turn plays the next
// steps in its file, and writes the session's transcript as Claude Code
// would, which is where the strip's numbers come from. `--chaos` keeps
// things moving: working agents finish, idle ones get new work, ask, or
// fail, and the questions get answered.
//
// Same `--seed`, same fleet: the people, machines, projects, tasks, models
// and fills are drawn from it (the stand-ins' output and the clock aren't).
// Nothing in it is anyone's: the people, logins and machines are made up,
// their avatars are drawn here (control serves them; it never asks GitHub),
// each machine's home, user and prompt are its persona's, and the agents'
// conversations live in that home, not yours.
//
// Left out: a hosted workspace. Its box is an `arugulad --hosted`, which
// does root's work through sudo on the machine it runs on (resolv.conf,
// reseeding, sealing), so it isn't something to start on a laptop.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { crc32, deflateSync } from "node:zlib";
import type { Device } from "./fixtures/device.ts";
import { writeStandIns } from "./e2e/stand-ins.ts";

const FAKE_ACP = resolve(import.meta.dirname, "../crates/daemon/tests/fake_acp.py");

export interface DemoOptions {
  seed: number;
  chaos: boolean;
  /** More agents and terminals on top of the set, for perf runs. */
  load: number;
}

/** `--demo [--seed N] [--chaos] [--load N]` out of `argv`: the options,
 * or null without `--demo`, and what's left. */
export function demoArgs(argv: string[]): { demo: DemoOptions | null; rest: string[] } {
  const rest: string[] = [];
  let on = false;
  const o: DemoOptions = { seed: 1, chaos: false, load: 0 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const num = (what: string) => {
      const v = Number(a.includes("=") ? a.split("=")[1] : argv[++i]);
      if (!Number.isInteger(v) || v < 0) throw new Error(`${what} wants a whole number`);
      return v;
    };
    if (a === "--demo") on = true;
    else if (a === "--chaos") o.chaos = true;
    else if (a === "--seed" || a.startsWith("--seed=")) o.seed = num("--seed");
    else if (a === "--load" || a.startsWith("--load=")) o.load = num("--load");
    else rest.push(a);
  }
  if (!on && (o.chaos || o.seed !== 1 || o.load)) throw new Error("--seed, --chaos and --load go with --demo");
  return { demo: on ? o : null, rest };
}

/** mulberry32: small, and the same everywhere. */
function rng(seed: number) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int: (lo: number, hi: number) => lo + Math.floor(next() * (hi - lo + 1)),
    pick: <T>(xs: readonly T[]): T => xs[Math.floor(next() * xs.length)],
    shuffle: <T>(xs: readonly T[]): T[] => {
      const out = [...xs];
      for (let i = out.length - 1; i > 0; i--) {
        const j = Math.floor(next() * (i + 1));
        [out[i], out[j]] = [out[j], out[i]];
      }
      return out;
    },
  };
}
type Rng = ReturnType<typeof rng>;

// ---- what the fleet is made of

/** Ordinary first names: no product-like ones. */
const PEOPLE = ["maya", "theo", "nina", "owen", "lena", "jonah", "clara", "eli", "ruth", "ivy", "felix", "hannah", "marcus", "grace", "leah", "tom"];
export const TEAM = "Acme";
const REPO = "acme/shop";
const REPO_ID = 4242;

/** The projects in each home (git repositories under ~/src), and what runs
 * in their terminals: stand-ins for these commands (e2e/stand-ins.ts). */
const PROJECTS: Record<string, string[]> = {
  shop: ["npm run dev", "vim src/cart.ts", ""],
  api: ["cargo build", "cargo test", ""],
  billing: ["pytest -x", "uvicorn app.main:app --reload", ""],
  infra: ["docker compose logs -f", "make plan", ""],
  worker: ["go test ./...", "go build ./cmd/worker", ""],
};
/** A build box's: builds, tests, logs. */
const BOX_WORK: [string, string][] = [
  ["api", "cargo build --release"],
  ["api", "cargo test"],
  ["worker", "go test ./..."],
  ["infra", "docker compose logs -f"],
  ["shop", "make"],
  ["billing", "tail -f log/worker.log"],
];

const OPUS = "claude-opus-5-5";
const OPUS_1M = "claude-opus-5-5[1m]";
const SONNET = "claude-sonnet-5-5";
const HAIKU = "claude-haiku-5-5";

type Step = Record<string, unknown>;
type Role = "working" | "near-full" | "permission" | "question" | "failed" | "compacted" | "overflow";

interface Task {
  prompt: string;
  title: string;
  /** Its steps as Claude Code's task list would have them (#445): each
   * step's words and its words while in progress. */
  plan: [string, string][];
  project: string;
  /** The first turn's steps, before the role's ending. */
  steps: Step[];
  /** What it runs, asks, or fails on: a command that fails, and why. */
  run?: string;
  question?: Step;
  fail?: { on: string; reason: string };
  summary: string;
}

const TASKS: Task[] = [
  {
    prompt: "Add retries with backoff to the webhook sender, and a test for the give-up path",
    title: "Webhook retries with backoff",
    plan: [["Read how the sender delivers", "Reading the sender"], ["Wrap delivery in retries with backoff", "Wrapping delivery in retries"], ["Test the give-up path", "Testing the give-up path"], ["Run the webhook tests", "Running the webhook tests"]],
    project: "api",
    steps: [
      { think: "The sender gives up on the first 5xx. I'll wrap the call and keep the attempt count on the delivery row." },
      { tool: "Read src/webhooks/sender.rs", kind: "read" },
      { tool: "Grep \"fn deliver\"", kind: "search" },
      { tool: "Edit src/webhooks/sender.rs", kind: "edit", ms: 600 },
      { tool: "Edit src/webhooks/tests.rs", kind: "edit", ms: 500 },
    ],
    run: "cargo test -p webhooks",
    summary: "Retries now back off 1s, 4s, 16s and give up after five; the new test covers the give-up path.",
  },
  {
    prompt: "Migrate the orders table to the new money columns without downtime",
    title: "Orders money columns",
    plan: [["Add the money columns", "Adding the money columns"], ["Backfill in batches of 5,000", "Backfilling in batches"], ["Switch reads behind ORDERS_MONEY_V2", "Switching reads over"], ["Drop the old columns", "Dropping the old columns"]],
    project: "billing",
    steps: [
      { think: "Two steps: add the new columns and backfill in batches, then switch reads over behind the flag." },
      { tool: "Read migrations/0041_orders.sql", kind: "read" },
      { tool: "Read app/models/order.py", kind: "read" },
      { tool: "Write migrations/0042_orders_money.sql", kind: "edit", ms: 700 },
    ],
    run: "alembic upgrade head",
    summary: "The backfill runs in batches of 5,000 and the reads switch over behind ORDERS_MONEY_V2.",
  },
  {
    prompt: "Bump Node to 22 in CI and fix whatever breaks",
    title: "Node 22 in CI",
    plan: [["Bump Node in the CI workflow", "Bumping Node in CI"], ["Update engines in package.json", "Updating engines"], ["Fix what breaks in the tests", "Fixing what breaks"]],
    project: "shop",
    steps: [
      { tool: "Read .github/workflows/ci.yml", kind: "read" },
      { tool: "Edit .github/workflows/ci.yml", kind: "edit" },
      { tool: "Edit package.json", kind: "edit" },
    ],
    run: "npm ci && npm test",
    summary: "CI runs on Node 22; one snapshot changed with the new Intl output.",
  },
  {
    prompt: "Product search is slow on big catalogs. Cache the results.",
    title: "Cache product search",
    plan: [["Profile the facet counts", "Profiling the facet counts"], ["Pick where results are cached", "Picking a cache"], ["Key results by catalog version", "Keying by catalog version"], ["Drop the cache on publish", "Dropping the cache on publish"]],
    project: "shop",
    steps: [
      { think: "The query itself is fine; it's the facet counts. Results can be cached per catalog version." },
      { tool: "Read src/search/products.ts", kind: "read" },
      { tool: "Grep \"facetCounts\"", kind: "search" },
    ],
    question: {
      question: "Where should search results be cached?",
      header: "Cache",
      multiSelect: false,
      options: [
        { label: "Redis", description: "Shared by every instance; one more service to run" },
        { label: "In-process LRU", description: "Nothing new to run; each instance warms its own" },
      ],
    },
    summary: "Search results are cached per catalog version and dropped when the catalog changes.",
  },
  {
    prompt: "The upload test fails about one run in ten. Find out why and fix it.",
    title: "Flaky upload test",
    plan: [["Reproduce the failure", "Reproducing the failure"], ["Give each test its own bucket", "Separating the buckets"], ["Run it 200 times in a row", "Running it 200 times"]],
    project: "api",
    steps: [
      { tool: "Read tests/upload.rs", kind: "read" },
      { tool: "cargo test upload -- --test-threads=1", kind: "execute", ms: 900 },
      { think: "It passes alone. Two tests share the temp bucket name; the clock decides which one wins." },
      { tool: "Edit tests/upload.rs", kind: "edit" },
    ],
    fail: {
      on: "cargo test -p upload",
      reason: "cargo test -p upload failed: upload::resumes_after_timeout timed out after 60s (1 of 48 tests failed)",
    },
    summary: "Each test gets a bucket of its own; 200 runs in a row passed.",
  },
  {
    prompt: "Write the release notes for 2.4 from the merged PRs",
    title: "Release notes for 2.4",
    plan: [["List the merged PRs", "Listing the merged PRs"], ["Group them by feature", "Grouping by feature"], ["Write CHANGELOG.md", "Writing the notes"]],
    project: "shop",
    steps: [
      { tool: "gh pr list --state merged --base main", kind: "execute" },
      { tool: "Read CHANGELOG.md", kind: "read" },
      { tool: "Edit CHANGELOG.md", kind: "edit", ms: 600 },
    ],
    summary: "The 2.4 notes are in CHANGELOG.md: saved carts, faster search, and the new refund flow.",
  },
  {
    prompt: "Refactor the auth middleware so each route says what it needs",
    title: "Per-route auth",
    plan: [["Map which routes need what", "Mapping the routes"], ["Add Public, Session and Admin", "Adding the route levels"], ["Check it in the middleware", "Checking in the middleware"], ["Remove the old guard", "Removing the old guard"]],
    project: "api",
    steps: [
      { tool: "Read src/auth/middleware.rs", kind: "read" },
      { tool: "Grep \"require_session\"", kind: "search" },
      { tool: "Edit src/auth/middleware.rs", kind: "edit", ms: 700 },
      { tool: "Edit src/routes/mod.rs", kind: "edit", ms: 500 },
    ],
    summary: "Routes declare Public, Session or Admin; the middleware checks it and the old guard is gone.",
  },
  {
    prompt: "Our Terraform plan shows drift on the queue. Reconcile it.",
    title: "Queue drift",
    plan: [["Plan the queue module", "Planning the queue module"], ["Find the hand-made change", "Finding the drift"], ["Put the timeout in the module", "Updating the module"], ["Apply and check the plan is clean", "Applying the module"]],
    project: "infra",
    steps: [
      { tool: "terraform plan -target=module.queue", kind: "execute", ms: 900 },
      { tool: "Read modules/queue/main.tf", kind: "read" },
      { tool: "Edit modules/queue/main.tf", kind: "edit" },
    ],
    run: "terraform apply -target=module.queue",
    summary: "The visibility timeout was changed by hand; it's in the module now and the plan is clean.",
  },
  {
    prompt: "Make the worker drain its queue before it shuts down",
    title: "Graceful worker shutdown",
    plan: [["Read the worker's main loop", "Reading the main loop"], ["Stop taking jobs on SIGTERM", "Handling SIGTERM"], ["Wait up to 30s for running jobs", "Draining running jobs"], ["Test the drain", "Testing the drain"]],
    project: "worker",
    steps: [
      { tool: "Read cmd/worker/main.go", kind: "read" },
      { tool: "Edit internal/pool/pool.go", kind: "edit", ms: 600 },
    ],
    run: "go test ./internal/pool/...",
    summary: "On SIGTERM the worker stops taking jobs and waits up to 30s for the ones it has.",
  },
];

/** Where each role's agent is in its task list (#445), so the plan board
 * has steps in every column: a step per entry, by the task's own steps;
 * `after` makes a pending one blocked on another (ids from 1). The
 * permission and question roles wait on you, so their step in progress
 * is in Review; the overflow role has no list. */
const PLANS: Record<Role, { status: "pending" | "in_progress" | "completed"; after?: string[] }[] | null> = {
  working: [{ status: "completed" }, { status: "in_progress" }, { status: "pending" }, { status: "pending" }],
  "near-full": [{ status: "completed" }, { status: "completed" }, { status: "in_progress" }, { status: "pending", after: ["3"] }],
  permission: [{ status: "completed" }, { status: "completed" }, { status: "in_progress" }, { status: "pending" }],
  question: [{ status: "completed" }, { status: "in_progress" }, { status: "pending", after: ["2"] }, { status: "pending" }],
  failed: [{ status: "completed" }, { status: "in_progress" }, { status: "pending", after: ["2"] }],
  compacted: [{ status: "completed" }, { status: "completed" }, { status: "completed" }, { status: "completed" }],
  overflow: null,
};

/** What each role's agent's context looks like (#397's strip). */
const FILLS: Record<Role, { model: string; usage: Step[] }> = {
  working: { model: OPUS, usage: [{ usage: 62_000 }] },
  "near-full": { model: OPUS, usage: [{ usage: 186_000 }] },
  permission: { model: SONNET, usage: [{ usage: 141_000 }] },
  question: { model: OPUS, usage: [{ usage: 44_000 }] },
  failed: { model: HAIKU, usage: [{ usage: 97_000 }] },
  // Compacted on its own at a quarter of a 1M window: not an overflow.
  compacted: { model: OPUS_1M, usage: [{ usage: 250_000 }, { compact: "auto", pre: 250_000, post: 20_000 }, { usage: 31_000 }] },
  // Compacted at 90% of the window (an overflow), then a long turn that
  // filled it again: the strip reads full, not the dip after compacting.
  overflow: { model: OPUS, usage: [{ usage: 181_000 }, { compact: "auto", pre: 181_000, post: 21_000 }, { usage: 166_000 }] },
};

const CHAT: { who: number; text: string }[] = [
  { who: 0, text: "Morning! 2.4 is cut from main; release notes are being drafted." },
  { who: 1, text: "The flaky upload test is back. I've got an agent on it." },
  { who: 2, text: "Search cache PR is up once we pick Redis or in-process. Opinions?" },
  { who: 0, text: "In-process for now. We can move it when we run more than three instances." },
  { who: 3, text: "Build box is green again after the disk cleanup 🎉" },
];
const DM = ["got a minute after standup? want to talk through the orders migration", "sure, 10:30?"];
const CHAOS_CHAT = [
  "Looks good, merging.",
  "Can someone look at the permission prompt on the build box?",
  "Agent finished the drift fix, plan is clean now.",
  "Heads up: deploying billing in 10.",
  "The upload test timed out on the build box again. Looking.",
  "Who has the release notes doc open? Adding the refund flow.",
];

const BOARD_TITLE = "Launch 2.4";
const BOARD_GOAL = "Shoppers can save carts and find products fast.\nThe 2.4 release goes out with notes.";
const DOC_TITLE = "Launch plan: 2.4";
const DOC = [
  { heading: "What ships", markdown: "- **Saved carts**: a cart outlives the session, for 30 days.\n- **Faster search**: results cached per catalog version.\n- **Refunds** from the order page, no support ticket." },
  { heading: "Risks", markdown: "| Risk | Owner | Plan |\n|---|---|---|\n| Orders migration locks the table | {b} | Backfill in batches, switch reads behind a flag |\n| Search cache serves stale prices | {c} | Key by catalog version; drop on publish |" },
  { heading: "Timeline", markdown: "1. Freeze on Tuesday.\n2. Staging soak Wednesday and Thursday.\n3. Release Friday morning, notes the same hour." },
];

// ---- the fleet

export interface DemoMachine {
  name: string;
  /** Whose laptop, or null for the team's build box. */
  owner: string | null;
  /** As dev-control takes it: `name` or `name@TEAM`. */
  spec: string;
}

interface Agent {
  machine: string;
  url: string;
  pane: number;
  file: string;
  turns: Step[][];
  task: Task;
  role: Role;
  model: string;
  tokens: number;
  window: number;
  /** What chaos thinks it's doing. */
  now: "working" | "asking" | "idle";
  since: number;
}

export class DemoFleet {
  readonly people: string[];
  readonly machines: DemoMachine[];
  private r: Rng;
  private agents: Agent[] = [];
  private urls = new Map<string, string>();
  private rows = new Map<string, string>();

  readonly opts: DemoOptions;
  readonly dir: string;

  constructor(opts: DemoOptions, dir: string) {
    this.opts = opts;
    this.dir = dir;
    this.r = rng(opts.seed);
    this.people = this.r.shuffle(PEOPLE).slice(0, 4);
    const [me, b, c] = this.people;
    this.machines = [
      { name: `${me}-laptop`, owner: me, spec: `${me}-laptop` },
      { name: `${b}-laptop`, owner: b, spec: `${b}-laptop@${TEAM}` },
      { name: `${c}-laptop`, owner: c, spec: `${c}-laptop@${TEAM}` },
      { name: "build-box", owner: null, spec: `build-box@${TEAM}` },
    ];
  }

  get viewer() {
    return this.people[0];
  }

  /** A machine's home: its persona's user, a neutral prompt, the stand-ins
   * first on PATH, its projects as git repositories, and Claude Code's
   * directory in it. Returns the daemon's extra arguments and environment. */
  daemon(name: string): { args: string[]; env: Record<string, string> } {
    const user = this.user(name);
    const home = this.home(name);
    const bin = join(home, ".local/bin");
    if (!existsSync(home)) {
      writeStandIns(bin);
      // Commands the stand-ins don't know, printing something plausible.
      const extra: Record<string, string> = {
        pytest: `#!/bin/bash\nwhile true; do echo "tests/test_orders.py::test_case_$((RANDOM % 90)) PASSED"; sleep 0.$((RANDOM % 9 + 1)); done\n`,
      };
      for (const [n, body] of Object.entries(extra)) writeFileSync(join(bin, n), body, { mode: 0o755 });
      for (const p of Object.keys(PROJECTS)) {
        const d = join(home, "src", p);
        mkdirSync(d, { recursive: true });
        execFileSync("git", ["init", "-q", "-b", "main", d]);
      }
      mkdirSync(join(home, ".claude"), { recursive: true });
      // The integration reads ~/.bashrc (crates/mux/src/shellint.rs).
      writeFileSync(
        join(home, ".bashrc"),
        `export USER=${user} LOGNAME=${user} HOSTNAME=${name}
PS1='\\[\\e]0;${user}@${name}: \\w\\a\\]\\[\\e[32m\\]${user}@${name}\\[\\e[0m\\]:\\[\\e[34m\\]\\w\\[\\e[0m\\]\\$ '
HISTFILE=~/.bash_history
`,
      );
    }
    return {
      args: ["--shell", "bash"],
      env: {
        HOME: home,
        USER: user,
        LOGNAME: user,
        SHELL: "/bin/bash",
        PATH: `${bin}:${process.env.PATH}`,
        CLAUDE_CONFIG_DIR: join(home, ".claude"),
        FAKE_ACP_DIR: join(home, ".fake-acp"),
        GIT_AUTHOR_NAME: user,
        GIT_AUTHOR_EMAIL: `${user}@example.com`,
        GIT_COMMITTER_NAME: user,
        GIT_COMMITTER_EMAIL: `${user}@example.com`,
      },
    };
  }

  /** A machine's daemon is up at `url`, which takes `token` (its
   * local-token) from programs. */
  at(name: string, url: string, token: string) {
    this.urls.set(name, url);
    tokens.set(url, token);
  }

  // ---- control: people, chat, the project's board, PR and Doc

  /** Avatars drawn here, kept fresh forever in control's cache, so control
   * never fetches a stranger's picture from GitHub for a made-up login. */
  avatars(db: string) {
    sql(db, (d) => {
      const put = d.prepare("INSERT OR REPLACE INTO chat_avatars (login, ctype, body, fetched) VALUES (?, 'image/png', ?, ?)");
      for (const p of this.people) put.run(p, identicon(`${this.opts.seed}:${p}`), 4_000_000_000_000);
    });
  }

  async chat(d: Device, others: Device[], team: string) {
    const all = [d, ...others];
    const general = await d.api<{ id: string }>(`/api/chat/teams/${team}/channels`, { name: "general" });
    // Everyone joins before saying anything (posting first is a 404).
    for (const o of others) await o.api(`/api/chat/rooms/${general.id}/join`, {});
    for (const m of CHAT) await all[m.who % all.length].api(`/api/chat/rooms/${general.id}/messages`, { text: m.text });
    if (others.length) {
      const dm = await others[1 % others.length].api<{ id: string }>(`/api/chat/teams/${team}/dms`, { with: d.account });
      await others[1 % others.length].api(`/api/chat/rooms/${dm.id}/messages`, { text: DM[0] });
      await d.api(`/api/chat/rooms/${dm.id}/messages`, { text: DM[1] });
    }
    return general.id;
  }

  /** The team's project: issues, a linked repo with an open PR (written
   * into control's database: there's no GitHub to mirror), a board with
   * rows in every lane, and a Doc. After [`panes`]: the Doing rows are the
   * working agents' tasks, and get their blocks in [`attach`]. */
  /** The machines may report their agent blocks' state to the team
   * project's boards. */
  async grants(d: Device, team: string, machineIds: Map<string, string>) {
    const { id: project } = await d.project(team);
    for (const id of machineIds.values())
      await d.api("/api/grants", { daemon: id, power: "boards.write", scope: { kind: "project", id: project } }).catch((e) => log(`grant: ${e}`));
  }

  async project(d: Device, others: Device[], team: string, db: string) {
    const { id: project } = await d.project(team);
    const people = [d, ...others];
    const who = (i: number) => people[i % people.length].account;
    const issue = async (title: string, assignee: number | null, body = "") =>
      (await d.api<{ issue: { id: string } } & { id?: string }>(`/api/projects/${project}/issues`, { title, body, labels: [], assignees: assignee === null ? [] : [who(assignee)] }));
    const idOf = (x: { issue?: { id: string }; id?: string }) => x.issue?.id ?? x.id!;
    const now = Date.now();
    sql(db, (s) => {
      s.prepare("INSERT OR IGNORE INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at) VALUES (?, ?, 0, ?, 1, ?)").run(REPO_ID, REPO, project, now);
      s.prepare(
        `INSERT OR IGNORE INTO forge_items (id, repo_id, number, kind, state, title, author_login, head_ref, head_sha, base_ref, created_at, updated_at, synced_at)
         VALUES ('fi_demo482', ?, 482, 'pr', 'open', 'Run CI on Node 22', ?, 'node-22', 'a1b2c3d', 'main', ?, ?, ?)`,
      ).run(REPO_ID, this.people[2], now - 3_600_000, now, now);
    });
    const board = await d.api<{ board: { id: string } }>(`/api/projects/${project}/boards`, { title: BOARD_TITLE, goal: BOARD_GOAL });
    const id = board.board.id;
    const row = async (pointer: Record<string, string>, note?: string) => {
      const v = await d.api<{ zones: { rows?: { id: string; pointer: { id?: string } }[] }[] }>(`/api/boards/${id}/entries`, { pointer, ...(note ? { note } : {}) }, "PUT");
      return v.zones.flatMap((z) => z.rows ?? []).find((r) => r.pointer.id === pointer.id)!.id;
    };
    // Next.
    for (const [t, a] of [["Saved carts survive a sign-out", 1], ["Rate-limit the public search API", null], ["Dark mode for the order page", 3]] as const)
      await row({ kind: "issue", id: idOf(await issue(t, a)) });
    // Doing: the working agents' tasks, each with its agent block.
    for (const [i, role] of (["working", "near-full"] as const).entries()) {
      const a = this.agents.find((x) => x.role === role);
      if (a) this.rows.set(role, await row({ kind: "issue", id: idOf(await issue(a.task.title, i)) }));
    }
    // Review: the open PR.
    const node = await row({ kind: "issue", id: idOf(await issue("Node 22 in CI", 2)) });
    await d.api(`/api/boards/${id}/attachments`, { row: node, pointer: { kind: "forge_item", id: "fi_demo482" }, role: "work" }).catch((e) => console.log(`  demo: PR row: ${e}`));
    // Done, one with its CI run as proof.
    for (const [t, a, proof] of [["Release notes for 2.4", 0, "https://ci.example.com/acme/shop/runs/1842"], ["Per-route auth", 3, null]] as const) {
      const i = idOf(await issue(t, a));
      const r = await row({ kind: "issue", id: i });
      await d.api(`/api/issues/${i}`, { state: "closed", reason: "completed" }, "PATCH");
      if (proof) await d.api(`/api/boards/${id}/attachments`, { row: r, pointer: { kind: "link", url: proof }, role: "proof" }).catch((e) => log(`proof: ${e}`));
    }
    const names = this.people;
    await d.api(`/api/projects/${project}/docs`, {
      title: DOC_TITLE,
      sections: DOC.map((s) => ({ heading: s.heading, markdown: s.markdown.replace("{b}", names[1]).replace("{c}", names[2]) })),
    });
    return { project, board: id };
  }

  /** The Doing rows' agents, on their rows. */
  async attach(d: Device, board: string, machineIds: Map<string, string>) {
    for (const [role, row] of this.rows) {
      const a = this.agents.find((x) => x.role === role);
      const machine = a && machineIds.get(a.machine);
      if (!a || !machine) continue;
      await d.api(`/api/boards/${board}/attachments`, { row, pointer: { kind: "block", machine, pane: String(a.pane) }, role: "work" }).catch((e) => console.log(`  demo: agent row: ${e}`));
    }
  }

  // ---- the machines: terminals and agents

  async panes() {
    const laptops = this.machines.filter((m) => m.owner);
    // Terminals: two or three projects' on each laptop, a build box's work.
    // Each machine's terminals share the tab its first shell is in, each
    // splitting the last, four to a tab.
    const split = async (machine: string, work: [string, string][]) => {
      const url = this.urls.get(machine)!;
      const first = (await paneIds(url))[0];
      // The machine's first shell, which starts at home, works in a project
      // too: a bare "~" isn't one.
      await typeIn(url, first, `cd ~/src/${work[0][0]} && clear`);
      let last: number | undefined = first;
      for (const [i, [p, cmd]] of work.entries()) last = await term(url, this.src(machine, p), cmd, (i + 1) % 4 ? last : undefined);
    };
    for (const m of laptops)
      await split(
        m.name,
        this.r.shuffle(Object.keys(PROJECTS)).slice(0, this.r.int(2, 3)).map((p) => [p, this.r.pick(PROJECTS[p])]),
      );
    await split("build-box", BOX_WORK);
    // Agents: one in each role, on machines the seed picks; the build box
    // gets the long one.
    // The roles that need something of their task (a question to ask, a
    // command to run) choose first.
    const roles: Role[] = ["question", "failed", "permission", "working", "near-full", "compacted", "overflow"];
    const tasks = this.r.shuffle(TASKS);
    const used = new Set<Task>();
    const fits = (t: Task, role: Role) =>
      role === "question" ? !!t.question : role === "failed" ? !!t.fail : ["permission", "working", "near-full"].includes(role) ? !!t.run : true;
    const take = (role: Role) => {
      const t = tasks.find((x) => !used.has(x) && fits(x, role)) ?? tasks.find((x) => !used.has(x)) ?? tasks[0];
      used.add(t);
      return t;
    };
    const where = (role: Role) => (role === "near-full" ? "build-box" : role === "working" ? laptops[0].name : this.r.pick(laptops).name);
    for (const role of roles) await this.agent(where(role), take(role), role);
    for (let i = 0; i < this.opts.load; i++) {
      const m = this.machines[i % this.machines.length].name;
      await term(this.urls.get(m)!, this.src(m, "api"), "cargo test");
      await this.agent(m, this.r.pick(TASKS), this.r.pick(roles), false);
    }
  }

  /** A project's directory on a machine. */
  private src(machine: string, project: string) {
    return join(this.home(machine), "src", project);
  }

  private user(machine: string) {
    return this.machines.find((x) => x.name === machine)?.owner ?? "ci";
  }

  /** A machine's home: `…/home/<user>`, so the swarm sees a home
   * directory in it (`~/src`), as it would in /home/<user>. */
  private home(machine: string) {
    return join(this.dir, "machines", machine, "home", this.user(machine));
  }

  /** An agent in `role` on `task`. The set's agents share a tab per
   * machine (each splits the last), so one screen shows them side by side. */
  private async agent(machine: string, task: Task, role: Role, split = true) {
    const url = this.urls.get(machine)!;
    const fill = FILLS[role];
    const window = fill.model.includes("[1m]") ? 1_000_000 : 200_000;
    const tokens = Number((fill.usage.at(-1) as { usage: number }).usage);
    const first: Step[] = [{ title: task.title }, ...task.steps, ...fill.usage.map((u) => ({ ...u, model: fill.model }))];
    // #445: its task list, as far as the role has got.
    const plan = PLANS[role];
    if (plan) first.push({ tasks: task.plan.slice(0, plan.length).map(([subject, active], i) => ({ subject, active, ...plan[i] })) });
    const busy = (cmd: string): Step => ({ busy: cmd, kind: "execute", secs: this.opts.chaos ? this.r.int(40, 150) : null });
    let now: Agent["now"] = "idle";
    switch (role) {
      case "working":
      case "near-full":
        first.push(busy(task.run ?? "cargo test"), { say: task.summary });
        now = "working";
        break;
      case "permission":
        first.push({ run: task.run ?? "npm test", output: "ok" }, { say: task.summary });
        now = "asking";
        break;
      case "question":
        first.push({ question: task.question ?? TASKS[3].question! }, { say: task.summary });
        now = "asking";
        break;
      case "failed":
        first.push(...failing(task.fail ?? TASKS[4].fail!));
        break;
      default:
        first.push({ say: task.summary, ms: 10 });
    }
    const file = join(this.dir, "agents", `${machine}-${this.agents.length}.json`);
    mkdirSync(join(this.dir, "agents"), { recursive: true });
    const turns = [first];
    writeFileSync(file, JSON.stringify({ turns, then: [{ say: "Done." }] }));
    const cwd = this.src(machine, task.project);
    const config = { agent: "claude", command: ["python3", FAKE_ACP, "--demo", file], cwd, prompt: task.prompt };
    const last = split ? this.agents.findLast((a) => a.machine === machine)?.pane : undefined;
    const pane = (await post(url, "/api/blocks", { type: "agent", config, ...(last ? { split: last } : {}) })).block as number;
    this.agents.push({ machine, url, pane, file, turns, task, role, model: fill.model, tokens, window, now, since: Date.now() });
  }

  // ---- chaos

  /** Keep things moving, forever: every few seconds one of these, drawn
   * from the seed: an idle agent gets new work (which runs a while, then
   * finishes), or asks to run something, or asks a question, or fails; a
   * waiting one gets its answer; a teammate says something in #general. */
  async chaos(people: Device[], general: string) {
    const r = rng(this.opts.seed * 7919 + 1);
    for (;;) {
      await sleep(r.int(6, 15) * 1000);
      try {
        await this.tick(r, people, general);
      } catch (e) {
        console.log(`  demo: chaos: ${String(e)}`);
      }
    }
  }

  private async tick(r: Rng, people: Device[], general: string) {
    const age = (a: Agent) => Date.now() - a.since;
    // A working one has finished when its busy step ran out: the block says.
    for (const a of this.agents.filter((x) => x.now !== "idle")) {
      const s = await state(a);
      if (s.status === "ready" && !s.pending?.length && !s.asks?.length) a.now = "idle";
    }
    const waiting = this.agents.filter((a) => a.now === "asking" && age(a) > 30_000);
    const idle = this.agents.filter((a) => a.now === "idle" && age(a) > 20_000);
    const roll = r.next();
    if (roll < 0.3 && waiting.length) {
      const a = r.pick(waiting);
      const s = await state(a);
      if (s.pending?.length) await post(a.url, `/api/blocks/${a.pane}/call/approve`, {});
      else if (s.asks?.length) {
        const q = s.asks[0] as { id: unknown };
        await post(a.url, `/api/blocks/${a.pane}/call/answer`, { id: q.id, content: { question_0: "In-process LRU" } });
      }
      a.now = "working";
      a.since = Date.now();
      return log(`${a.machine}: answered "${a.task.title}"`);
    }
    if (roll < 0.85 && idle.length) {
      const a = r.pick(idle);
      const what = r.pick(["work", "work", "ask", "question", "fail"] as const);
      const task = r.pick(TASKS.filter((t) => (what === "question" ? t.question : what === "fail" ? t.fail : what === "ask" ? t.run : true)));
      const steps: Step[] = [{ title: task.title }, ...task.steps, ...this.grow(a, r)];
      if (what === "work") steps.push({ busy: task.run ?? "cargo test", kind: "execute", secs: r.int(30, 120) }, { say: task.summary });
      if (what === "ask") steps.push({ run: task.run ?? "npm test", output: "ok" }, { say: task.summary });
      if (what === "question") steps.push({ question: task.question! }, { say: task.summary });
      if (what === "fail") steps.push(...failing(task.fail!));
      a.turns.push(steps);
      a.task = task;
      writeFileSync(a.file, JSON.stringify({ turns: a.turns, then: [{ say: "Done." }] }));
      await post(a.url, `/api/blocks/${a.pane}/call/send`, { text: task.prompt });
      a.now = what === "ask" || what === "question" ? "asking" : what === "fail" ? "idle" : "working";
      a.since = Date.now();
      return log(`${a.machine}: ${what} "${task.title}"`);
    }
    if (people.length > 1) {
      const who = people[r.int(1, people.length - 1)];
      await who.api(`/api/chat/rooms/${general}/messages`, { text: r.pick(CHAOS_CHAT) });
    }
  }

  /** A turn's growth in context: more tokens, and an automatic compaction
   * once the window passes 75%, below where it would be an overflow (80%):
   * an overflow's strip reads low right after it, which looks wrong. */
  private grow(a: Agent, r: Rng): Step[] {
    const next = a.tokens + r.int(8_000, 30_000);
    if (next > a.window * 0.75) {
      const pre = Math.min(next, Math.floor(a.window * 0.78));
      const post = r.int(15_000, 25_000);
      a.tokens = post + r.int(2_000, 8_000);
      return [
        { usage: pre, model: a.model },
        { compact: "auto", pre, post },
        { usage: a.tokens, model: a.model },
      ];
    }
    a.tokens = next;
    return [{ usage: next, model: a.model }];
  }
}

// ---- helpers

/** A turn that ends on a failing command: the tool call fails, then the
 * turn does, saying why. */
const failing = (f: { on: string; reason: string }): Step[] => [
  { tool: f.on, kind: "execute", ms: 1200, status: "failed" },
  { fail: f.reason },
];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const log = (s: string) => console.log(`  demo: ${s}`);

/** Each daemon's local token, by its URL. */
const tokens = new Map<string, string>();
const auth = (base: string) => ({ Authorization: `Bearer ${tokens.get(base) ?? ""}` });

async function post(base: string, path: string, body: unknown): Promise<Record<string, unknown>> {
  const res = await fetch(base + path, { method: "POST", headers: { "Content-Type": "application/json", ...auth(base) }, body: JSON.stringify(body) });
  if (!res.ok) throw new Error(`${path}: ${res.status} ${await res.text()}`);
  return (await res.json()) as Record<string, unknown>;
}

async function state(a: Agent): Promise<{ status: string; pending?: unknown[]; asks?: unknown[] }> {
  return (await post(a.url, `/api/blocks/${a.pane}/call/state`, {})) as never;
}

/** The panes a daemon has, first to last. */
async function paneIds(url: string): Promise<number[]> {
  return ((await (await fetch(`${url}/api/panes`, { headers: auth(url) })).json()) as { id: number }[]).map((p) => p.id);
}

/** A shell in `cwd`, running `line` once its prompt is up; splitting
 * `split`, or in a tab of its own. */
async function term(url: string, cwd: string, line: string, split?: number) {
  const pane = (await post(url, "/api/run", { cwd, ...(split ? { split } : {}) })).pane as number;
  if (line) await typeIn(url, pane, line);
  return pane;
}

/** Type `line` and Enter into a shell once its prompt is up. */
async function typeIn(url: string, pane: number, line: string) {
  for (let i = 0; i < 50; i++) {
    const info = ((await (await fetch(`${url}/api/panes`, { headers: auth(url) })).json()) as { id: number; running: boolean }[]).find((p) => p.id === pane);
    if (info?.running) break;
    await sleep(100);
  }
  await sleep(300);
  await post(url, `/api/panes/${pane}/send`, { text: line, enter: true });
}

function sql<T>(file: string, f: (db: DatabaseSync) => T): T {
  const db = new DatabaseSync(file);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    return f(db);
  } finally {
    db.close();
  }
}

/** A 128-px identicon for `key`: a 5×5 mirrored pattern in one colour on
 * a light ground, as GitHub draws a default avatar. PNG, drawn here. */
export function identicon(key: string): Buffer {
  let h = 2166136261;
  for (const c of key) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0;
  const r = rng(h);
  const [cr, cg, cb] = hsl(r.next() * 360, 0.55, 0.55);
  const on: boolean[][] = Array.from({ length: 5 }, () => {
    const left = [r.next() < 0.5, r.next() < 0.5, r.next() < 0.5];
    return [left[0], left[1], left[2], left[1], left[0]];
  });
  const size = 128;
  const pad = 14;
  const cell = (size - 2 * pad) / 5;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    for (let x = 0; x < size; x++) {
      const gx = Math.floor((x - pad) / cell);
      const gy = Math.floor((y - pad) / cell);
      const ink = gx >= 0 && gx < 5 && gy >= 0 && gy < 5 && on[gy][gx];
      const o = y * (size * 4 + 1) + 1 + x * 4;
      raw[o] = ink ? cr : 240;
      raw[o + 1] = ink ? cg : 240;
      raw[o + 2] = ink ? cb : 240;
      raw[o + 3] = 255;
    }
  }
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(td) >>> 0);
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

function hsl(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255)];
}
