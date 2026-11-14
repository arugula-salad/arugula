// The hub's `GET /api/projects/{project}/sessions/by-card`, as the cloud
// answers it (closed source, so its shape is written down here): a session
// on a machine with its panes, an agent block, a cloud workspace's session,
// and the sessions on no card last; the cards' boards and lanes, the
// sessions' branches and a hosted one's `settled` (cloud#87). For the tests of what reads it
// (src/shared/project-sessions.ts).

import type { SessionsByCard } from "../src/shared/project-sessions.ts";

export const PROJECT = "prj_0123456789abcdef0123456789abcdef";

export const BY_CARD = {
  project: PROJECT,
  groups: [
    {
      card: { id: "iss_983", number: 983, title: "L2: the platform knows each session's project and card", board: { id: "brd_1", title: "Inversion" }, lane: "doing" },
      sessions: [
        {
          kind: "machine",
          id: "7",
          name: "i983",
          on: { kind: "machine", id: "m_ws", name: "workstation" },
          panes: [
            { pane: 12, kind: "agent", work_item: "iss_983", state: "needs_input", branch: "arugula/983-l2" },
            { pane: 13, kind: "terminal", work_item: null, state: null, branch: null },
          ],
          branch: null,
          updated_at: 1794000000000,
        },
        { kind: "hosted", id: "hs_1", name: "arugula/983-cloud-half", on: { kind: "hosted" }, state: "working", settled: false, branch: "arugula/983-cloud-half", updated_at: 0 },
      ],
    },
    {
      card: { id: "iss_987", board: null, lane: null },
      sessions: [{ kind: "agent", id: 4, name: "Claude · #987 drawer", on: { kind: "machine", id: "m_b1", name: "build-01" }, state: "idle", branch: null, updated_at: 0 }],
    },
    {
      card: null,
      sessions: [{ kind: "machine", id: "2", name: "release", on: { kind: "machine", id: "m_gone", name: "laptop" }, panes: [{ pane: 3, kind: "terminal", work_item: null, state: null }], updated_at: 0 }],
    },
  ],
} satisfies SessionsByCard;
