// The made-up team the site's pictures show (`just screenshots`): four
// people building Game night, an app to plan board game nights, with three
// boards, their tickets, a few rooms of chat and machines of their own.
// screenshots/app-page.tsx answers the platform's routes from here; the
// specs read the ids. Every name is made up.

import type { BoardView, ChatMessage, ChatRoom, NativeIssue, ProjectBoard } from "../src/shared/proto";

/** The team's project; Priya's own. */
export const P = "prj_9a3e0002";
export const PERSONAL = "prj_9a3e0001";
export const TEAM = "tm_9a3e";
export const ME = "acct_priya";
/** The shots' daemon (priya-mbp) as control would name the machine. */
export const HERE = "m_9a3e00priya";

export const PEOPLE = [
  { account: ME, name: "Priya", role: "owner" },
  { account: "acct_marcus", name: "Marcus", role: "editor" },
  { account: "acct_jen", name: "Jen", role: "editor" },
  { account: "acct_tomas", name: "Tomás", role: "editor" },
] as const;
const nameOf = (a: string) => PEOPLE.find((p) => p.account === a)?.name ?? a;

export const LAUNCH = "brd_6a3e0001";
export const IOS = "brd_6a3e0002";
export const LIBRARY = "brd_6a3e0003";

type Lane = "next" | "doing" | "review" | "done";
/** A ticket: its number, title, lane, labels and who has it. */
type Ticket = [number, string, Lane, string[], string[]];

const TICKETS: Record<string, { title: string; goal: string; tickets: Ticket[] }> = {
  [LAUNCH]: {
    title: "Launch v1",
    goal: "Friends can plan a game night end to end: invite, RSVP, vote on games.",
    tickets: [
      [3, "Sign in with a magic link", "done", ["auth"], [ME]],
      [4, "Create a game night with a date and a place", "done", ["web"], ["acct_jen"]],
      [6, "Invite friends with a link", "done", ["web"], ["acct_jen"]],
      [7, "RSVP: yes, no, maybe", "done", ["web"], ["acct_marcus"]],
      [9, "Vote on which games to play", "review", ["web"], ["acct_jen"]],
      [14, "Fix the flaky session expiry test", "doing", ["auth", "bug"], [ME]],
      [11, "Reminder email the day before", "doing", ["email"], ["acct_marcus"]],
      [12, "Snack sign-up list", "doing", ["web"], ["acct_tomas"]],
      [15, "Download a calendar invite (.ics)", "next", ["web"], []],
      [16, "Dark mode", "next", ["design"], []],
    ],
  },
  [IOS]: {
    title: "iPhone app",
    goal: "The same game night, in your pocket, with a push when friends answer.",
    tickets: [
      [20, "TestFlight build for the group", "done", ["ios"], ["acct_tomas"]],
      [21, "Push a notification on each RSVP", "doing", ["ios"], ["acct_tomas"]],
      [22, "Share an invite from the share sheet", "next", ["ios"], []],
    ],
  },
  [LIBRARY]: {
    title: "Game library",
    goal: "Everyone's shelf in one place, so the vote only offers games someone owns.",
    tickets: [
      [31, "Import a BoardGameGeek collection", "doing", ["library"], ["acct_marcus"]],
      [32, "Filter games by player count", "review", ["library"], ["acct_jen"]],
      [33, "Box art for every game", "next", ["design"], []],
      [30, "A shelf per person", "done", ["library"], ["acct_marcus"]],
    ],
  },
};

/** Where Priya's own agent works: its card, on Launch v1. */
export const MINE = { board: LAUNCH, row: "row_14", number: 14, title: "Fix the flaky session expiry test" };

const HOUR = 3_600_000;
const progress = (t: Ticket[]) => ({
  done: t.filter((x) => x[2] === "done").length,
  total: t.length,
  proven: 0,
  review: t.filter((x) => x[2] === "review").length,
  doing: t.filter((x) => x[2] === "doing").length,
});

/** The boards as `GET /api/projects/P/boards` lists them. `lead`: the
 * session that leads Launch v1, on this machine (its name, once known). */
export function boards(now: number, lead?: { machine: string; name: string; session: string; pane?: number }): ProjectBoard[] {
  return Object.entries(TICKETS).map(([id, b], i) => ({
    id,
    project: P,
    title: b.title,
    goal: b.goal,
    created_by: ME,
    created_at: now - (30 - i) * 24 * HOUR,
    updated_at: now - (i + 1) * HOUR,
    done_note: "",
    closed_by: "",
    progress: progress(b.tickets),
    ...(id === LAUNCH && lead ? { lead: { machine: lead.machine, machine_name: lead.name, session: lead.session, ...(lead.pane === undefined ? {} : { pane: lead.pane }), at: now - 2 * HOUR } } : {}),
  }));
}

const LANES: Lane[] = ["next", "doing", "review", "done"];

/** A PR on a card in Review: open, its checks green. */
const PRS: Record<number, number> = { 9: 41, 32: 44 };

/** A board's view, as `GET /api/boards/:id` answers. */
export function view(id: string, now: number, lead?: { machine: string; name: string; session: string; pane?: number }): BoardView {
  const b = TICKETS[id];
  const board = boards(now, lead).find((x) => x.id === id)!;
  const row = ([n, title, lane, labels, assignees]: Ticket, i: number) => ({
    id: `row_${n}`,
    rev: 1,
    pointer: { kind: "issue" as const, id: `iss_${n}` },
    note: "",
    status: "none" as const,
    lane,
    attachments: PRS[n]
      ? [{ id: `att_${n}`, role: "work" as const, pointer: { kind: "forge_item" as const, id: `gamenight/app#${PRS[n]}` }, live: { state: "open" as const, kind: "pr", number: PRS[n], title: `${title}`, repo: "gamenight/app" } }]
      : [],
    live: { state: "open" as const, kind: "native", number: n, title, labels, assignees: assignees.map(nameOf) },
    created_at: now - (40 - i) * HOUR,
    updated_at: now - i * HOUR,
  });
  return {
    board,
    zones: LANES.map((l) => ({ id: `lane_${l}`, rev: 1, title: l[0].toUpperCase() + l.slice(1), kind: "lane" as const, rows: b.tickets.map(row).filter((r) => r.lane === l) })),
    groups: [],
    docs: [],
  };
}

/** The team's issues, as `GET /api/projects/P/issues` lists them. */
export function issues(now: number, state: string): NativeIssue[] {
  const extra: Ticket[] = [
    [34, "Show who's bringing which game", "next", ["library"], []],
    [35, "Time zones in the reminder email are wrong for Lisbon", "next", ["email", "bug"], ["acct_marcus"]],
    [36, "Let the host close the vote", "next", ["web"], []],
  ];
  const all = [...Object.values(TICKETS).flatMap((b) => b.tickets), ...extra];
  return all
    .map(([n, title, lane, labels, assignees]) => ({
      id: `iss_${n}`,
      project: P,
      number: n,
      title,
      body: "",
      state: lane === "done" ? "closed" : "open",
      close_reason: lane === "done" ? "completed" : null,
      author: PEOPLE[n % PEOPLE.length].account,
      agent: null,
      labels,
      assignees,
      github: null,
      created_at: now - (60 - n) * 6 * HOUR,
      updated_at: now - (n % 7) * HOUR - n * 60_000,
      closed_at: lane === "done" ? now - n * HOUR : null,
    }))
    .filter((i) => state === "all" || i.state === state)
    .sort((a, b) => b.updated_at - a.updated_at);
}

export const LABELS = ["auth", "bug", "design", "email", "ios", "library", "web"];

// Chat: three channels and a DM, the newest first.
type Said = [string, string, number, { agent?: string; replies?: number; react?: [string, number] }?];
export const ROOMS: { id: string; name: string; kind: "channel" | "dm"; topic: string; with?: string; said: Said[] }[] = [
  {
    id: "ch_9a3e0001",
    name: "game-night",
    kind: "channel",
    topic: "Building Game night · launch is the 12th",
    said: [
      ["acct_jen", "Voting is up for review: #9. You can change your vote until the host closes it.", 190],
      ["acct_marcus", "Nice. I'll try it on my phone tonight", 170, { react: ["👍", 2] }],
      [ME, "My agent found the flaky session test: it slept for the TTL instead of moving a clock. Fix is on #14, waiting on me to approve the commit.", 95, { replies: 3 }],
      ["acct_marcus", "Reminder emails go out at 9am in the host's time zone. Lisbon is wrong though, filed #35", 64],
      ["acct_tomas", "TestFlight build 0.4 is out to all of you. Push on RSVP is next", 41, { react: ["🎉", 3] }],
      ["acct_marcus", "Agent on #31 has the BGG import parsing 400 games from my collection in 2s.", 22, { agent: "marcus-mbp" }],
      ["acct_jen", "Who's hosting Thursday? I can bring Wingspan", 9],
      [ME, "Me! 7pm. Let's test the whole thing for real", 4, { react: ["🎲", 4] }],
    ],
  },
  {
    id: "ch_9a3e0002",
    name: "deploys",
    kind: "channel",
    topic: "Staging and prod",
    said: [
      ["acct_tomas", "Staging is on 0.4.2", 300],
      ["acct_marcus", "Migrations for the snack list ran clean", 120],
    ],
  },
  { id: "ch_9a3e0003", name: "design", kind: "channel", topic: "Screens, colours and copy", said: [["acct_jen", "New box-art tiles in Figma", 400]] },
  { id: "dm_9a3e0004", name: "Marcus", kind: "dm", topic: "", with: "acct_marcus", said: [["acct_marcus", "Can you look at #11 when the agent's done?", 30]] },
];

/** A room as `GET /api/chat` lists it. */
export function room(r: (typeof ROOMS)[number]): ChatRoom {
  const last = r.said.length;
  return {
    id: r.id,
    kind: r.kind,
    team: TEAM,
    name: r.name,
    machine: null,
    machine_name: null,
    target: null,
    project: P,
    with: r.with ? { account: r.with, name: nameOf(r.with), dnd: false } : null,
    others: r.with ? [{ account: r.with, name: nameOf(r.with), dnd: false }] : [],
    private: false,
    last,
    read: r.id === ROOMS[0].id ? last : last - 1,
    unread: r.id === ROOMS[0].id ? 0 : 1,
    mentions: r.kind === "dm" ? 1 : 0,
    wants: 0,
    muted: false,
    level: "all",
    level_own: false,
    archived: false,
    writable: true,
    topic: r.topic,
    description: "",
    pins: 0,
    topic_writable: true,
    staff: false,
  };
}

/** A room's messages, as `GET /api/chat/rooms/:id/messages` answers. */
export function messages(r: (typeof ROOMS)[number], now: number): ChatMessage[] {
  return r.said.map(([author, text, minsAgo, more], i) => ({
    seq: i + 1,
    at: now - minsAgo * 60_000,
    author,
    name: more?.agent ? `${nameOf(author)}'s agent on ${more.agent}` : nameOf(author),
    text,
    quote: null,
    pane: null,
    mentions: [],
    edited: null,
    deleted: null,
    imported: null,
    agent: more?.agent ? { machine: `m_${more.agent}`, machine_name: more.agent } : null,
    reactions: more?.react ? [{ emoji: more.react[0], count: more.react[1], agents: 0, mine: false, who: PEOPLE.slice(0, more.react[1]).map((p) => ({ account: p.account, name: p.name, agent: null })) }] : [],
    files: [],
    pinned: false,
    also: false,
    ...(more?.replies ? { thread: { replies: more.replies, last: now - 60_000 * 30, repliers: [{ account: "acct_marcus", name: "Marcus", dnd: false }, { account: "acct_jen", name: "Jen", dnd: false }], following: true, read: more.replies, unread: 0, mentions: 0 } } : {}),
  }));
}

/** Teammates' machines, as their summaries would come to the page's
 * fleet: each a session in the project with an agent on a ticket, and a
 * shell or two beside it. `[pane, ticket, what it's doing, attention]`. */
export const MACHINES: { name: string; id: string; owner?: string; online: boolean; agents: [number, number, string, string][]; shells: [number, string][] }[] = [
  {
    name: "marcus-mbp",
    id: "a11a00000000aa01",
    owner: "Marcus",
    online: true,
    agents: [
      [3, 11, "Reminder email the day before", "working"],
      [7, 31, "Import a BoardGameGeek collection", "working"],
    ],
    shells: [[4, "npm run dev"], [8, "cargo test -p library"]],
  },
  {
    name: "jen-studio",
    id: "a11a00000000aa02",
    owner: "Jen",
    online: true,
    agents: [
      [2, 9, "Vote on which games to play", "idle"],
      [5, 32, "Filter games by player count", "working"],
    ],
    shells: [[1, "vite"]],
  },
  {
    name: "tomas-xps",
    id: "a11a00000000aa03",
    owner: "Tomás",
    online: true,
    agents: [
      [4, 12, "Snack sign-up list", "working"],
      [6, 21, "Push a notification on each RSVP", "working"],
    ],
    shells: [[2, "xcodebuild test"]],
  },
  { name: "build-box", id: "a11a00000000aa04", online: true, agents: [], shells: [[1, "tail -f deploy.log"], [2, "docker compose logs -f"]] },
];

/** Where each ticket is, for a fake agent's `on_issue`. */
export function cardOf(n: number): { board: string; row: string; title: string } | null {
  for (const [id, b] of Object.entries(TICKETS)) if (b.tickets.some((t) => t[0] === n)) return { board: id, row: `row_${n}`, title: b.title };
  return null;
}
