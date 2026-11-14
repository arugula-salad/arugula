// A project's sessions as the cloud answers them (Arugula inversion L3),
// for scripts/sessions-by-card.test.mjs and sessions-page.spec.ts: the
// by-card route (`GET /api/projects/{p}/sessions/by-card`, merged in the
// cloud but not deployed when this was written) and the hosted wave
// (`GET /api/projects/{p}/sessions`) beside it. Typed, so a change to
// either shape fails the typecheck here.

import type { SessionsByCard } from "../src/shared/project-sessions";
import type { SessionRow, SessionWave } from "../src/shared/proto.gen";

export const PROJECT = "prj_aaaa0002";
/** A teammate's computer. */
export const DANA = "beefcafe01";

const issue = (id: string, number: number, title: string) => ({ id, number, title, labels: ["inversion"], state: "open" });

const hosted = (session: string, over: Partial<SessionRow>): SessionRow => ({
  session,
  project: PROJECT,
  room: `room_${session}`,
  parent: 1,
  state: "working",
  settled: false,
  role: "thread",
  issue: null,
  asker: "acct_a",
  asker_name: "Sam",
  started_by: null,
  agent: null,
  workspace: "ws_1",
  workspace_name: "arugula-dev",
  repo: "arugula-salad/arugula",
  branch: "arugula/aaaa-work",
  head: "abc1234",
  pushed_at: 1794000000000,
  waiting: null,
  question: null,
  pr: null,
  merged_at: null,
  reason: null,
  created_at: 1794000000000,
  updated_at: 1794000000000,
  ...over,
});

/** The hosted wave: one waiting on you on #985, one finished and one whose
 * turn ended (`done`, not finished) on #970, and one on #990 the by-card
 * answer hasn't yet. */
export const wave = (machine: string): SessionWave => ({
  project: PROJECT,
  sessions: [
    hosted("ses_0985aaaa", { issue: issue("iss_985", 985, "A project's Sessions page"), branch: "arugula/0985-sessions", state: "needs_input", settled: true, waiting: "its question", question: "Which lane?" }),
    hosted("ses_0970bbbb", { issue: issue("iss_970", 970, "The rail"), branch: "arugula/0970-rail", state: "stopped", settled: true }),
    hosted("ses_0971dddd", { issue: issue("iss_970", 970, "The rail"), branch: "arugula/0970-rail-turn", state: "done", pushed_at: 1794000200000 }),
    hosted("ses_0990cccc", { issue: issue("iss_990", 990, "Fold finished"), branch: "arugula/0990-fold", started_by: `Sam's agent on ${machine}` }),
  ],
  labels: ["inversion"],
  askers: [["acct_a", "Sam"]],
  tracker: null,
  more: false,
});

/** Every session in the project: `machine` is the reader's own computer,
 * whose pane `pane` is an agent on #970; Dana's computer has a release
 * terminal on no card. */
export const byCard = (machine: string, pane: number): SessionsByCard => ({
  project: PROJECT,
  groups: [
    {
      card: { id: "iss_970", number: 970, title: "The rail" },
      sessions: [
        {
          kind: "machine",
          id: "7",
          name: "rail-work",
          on: { kind: "machine", id: machine, name: "sams-laptop" },
          panes: [{ pane, kind: "terminal", work_item: "iss_970", state: "working" }],
          updated_at: 1794000500000,
        },
        { kind: "hosted", id: "ses_0970bbbb", name: "arugula/0970-rail", on: { kind: "hosted" }, state: "stopped", updated_at: 1794000000000 },
        { kind: "hosted", id: "ses_0971dddd", name: "arugula/0970-rail-turn", on: { kind: "hosted" }, state: "done", updated_at: 1794000200000 },
      ],
    },
    {
      card: { id: "iss_985", number: 985, title: "A project's Sessions page" },
      sessions: [{ kind: "hosted", id: "ses_0985aaaa", name: "arugula/0985-sessions", on: { kind: "hosted" }, state: "needs_input", updated_at: 1794000000000 }],
    },
    {
      card: null,
      sessions: [
        {
          kind: "machine",
          id: "2",
          name: "release",
          on: { kind: "machine", id: DANA, name: "danas-mini" },
          panes: [{ pane: 3, kind: "terminal", work_item: null, state: null }],
          updated_at: 1794000100000,
        },
        { kind: "agent", id: 9, name: "lead", on: { kind: "machine", id: DANA, name: "danas-mini" }, state: "exited", updated_at: 1794000000000 },
      ],
    },
  ],
});
