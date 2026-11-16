// A project's sessions as the cloud answers them (Arugula inversion L3),
// for scripts/row-sessions.test.mjs and board-sessions.spec.ts: the
// by-card route (`GET /api/projects/{p}/sessions/by-card`). Typed, so a
// change to its shape fails the typecheck here.

import type { SessionsByCard } from "../src/shared/project-sessions";

export const PROJECT = "prj_aaaa0002";
/** A teammate's computer. */
export const DANA = "beefcafe01";

/** Every session in the project: `machine` is the reader's own computer,
 * whose pane `pane` is an agent on #970; Dana's computer has a release
 * terminal on no card. #985 is on the board Launch, in Doing; #970 is on
 * no board. `fields` false is a cloud from before the answer said cards'
 * boards and lanes, sessions' branches and hosted ones' `settled`. */
export const byCard = (machine: string, pane: number, fields = true): SessionsByCard => {
  const answer: SessionsByCard = {
    project: PROJECT,
    groups: [
      {
        card: { id: "iss_970", number: 970, title: "The rail", board: null, lane: null },
        sessions: [
          {
            kind: "machine",
            id: "7",
            name: "rail-work",
            on: { kind: "machine", id: machine, name: "sams-laptop" },
            panes: [{ pane, kind: "terminal", work_item: "iss_970", state: "working", branch: "arugula/0970-rail-pane" }],
            branch: null,
            updated_at: 1794000500000,
          },
          { kind: "hosted", id: "ses_0970bbbb", name: "arugula/0970-rail", on: { kind: "hosted" }, state: "stopped", settled: true, branch: "arugula/0970-rail", updated_at: 1794000000000 },
          { kind: "hosted", id: "ses_0971dddd", name: "arugula/0970-rail-turn", on: { kind: "hosted" }, state: "done", settled: true, branch: "arugula/0970-rail-turn", updated_at: 1794000200000 },
        ],
      },
      {
        card: { id: "iss_985", number: 985, title: "A project's Sessions page", board: { id: "brd_00e2e0", title: "Launch" }, lane: "doing" },
        sessions: [{ kind: "hosted", id: "ses_0985aaaa", name: "arugula/0985-sessions", on: { kind: "hosted" }, state: "needs_input", settled: true, branch: "arugula/0985-sessions", updated_at: 1794000000000 }],
      },
      {
        card: null,
        sessions: [
          {
            kind: "machine",
            id: "2",
            name: "release",
            on: { kind: "machine", id: DANA, name: "danas-mini" },
            panes: [{ pane: 3, kind: "terminal", work_item: null, state: null, branch: null }],
            branch: null,
            updated_at: 1794000100000,
          },
          { kind: "agent", id: 9, name: "lead", on: { kind: "machine", id: DANA, name: "danas-mini" }, state: "exited", branch: "main", updated_at: 1794000000000 },
        ],
      },
    ],
  };
  if (fields) return answer;
  return JSON.parse(JSON.stringify(answer, (k, v) => (["board", "lane", "branch", "settled"].includes(k) ? undefined : v)));
};
