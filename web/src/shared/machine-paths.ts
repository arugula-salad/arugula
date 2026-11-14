// Where the machines are in the app (Arugula #989): under a project, at
// `/p/<project>/machines…`, or project-less at `/machines…` for the app to
// put in the current project. And the way between those and the shell's
// addresses on control's page (`#pane=`, `#machine=`, `#swarm…`): the app's
// machine pages fall back to control's page until the hub reaches machines
// from the app's host, and control's page sends its links to the app once
// it does. Shared, since both pages read it.

/** A place among the machines. */
export type MachineAt =
  /** The machines. `pane` (a notification's, naming no machine) opens on
   * the home or last machine; `name`, a machine by name (control's old
   * `?machine=NAME`). `layer`: one of the shell's own places over the
   * machine shown (`overview`, `agents`, `settings=machine`…). */
  | { at: "machines"; pane?: number; name?: string; layer?: string }
  /** A machine's page, with a pane to show: shared from (`share`), a person
   * to follow there (`follow`), alone in its window (`pop`); or with
   * Start an agent open (`agent`). */
  | { at: "machine"; machine: string; pane?: number; share?: true; follow?: string; pop?: true; agent?: true }
  /** Every pane on every machine (decision 3: under Machines), at a pane's
   * card (`focus`) or on one board's agents. */
  | { at: "swarm"; focus?: { machine?: string; pane: number }; board?: string };

/** A machine's id at the front of a path segment, before its slug: a
 * daemon's id is hex. */
const MACHINE = /^([0-9a-f]{6,64})(?:-.*)?$/;
const BOARD = /^brd_[\w-]+$/;
/** The shell's own places over a machine, as their fragments say them. */
const LAYER = /^(?:overview(?:=\d+)?|agents(?:=team)?|settings(?:=[\w-]+)?|developer|getting-started=\w+)$/;
/** Those of them control's page sends on to the app: the machine's own.
 * Your account's, your devices' and the rest of Settings stay with
 * control's sign-in and passkeys, and so does Getting started (joining a
 * machine to control). */
const MOVES = /^(?:overview(?:=\d+)?|agents(?:=team)?|settings=machine|developer)$/;

const num = (v: string | null) => (v !== null && /^\d{1,9}$/.test(v) ? Number(v) : undefined);

/** The place `rest` names (the path's segments after `machines`), or null. */
export function parseMachines(rest: string[], q: URLSearchParams, hash = ""): MachineAt | null {
  if (rest.length === 0) {
    const pane = num(q.get("pane"));
    const name = q.get("machine");
    const layer = hash.replace(/^#/, "");
    return { at: "machines", ...(pane !== undefined ? { pane } : {}), ...(name ? { name } : {}), ...(LAYER.test(layer) ? { layer } : {}) };
  }
  if (rest.length !== 1) return null;
  if (rest[0] === "swarm") {
    const f = /^(?:([0-9a-f]+)\.)?(\d{1,9})$/.exec(q.get("focus") ?? "");
    const board = q.get("board");
    return { at: "swarm", ...(f ? { focus: { ...(f[1] ? { machine: f[1] } : {}), pane: Number(f[2]) } } : {}), ...(board && BOARD.test(board) ? { board } : {}) };
  }
  const machine = MACHINE.exec(rest[0])?.[1];
  if (!machine) return null;
  const pane = num(q.get("pane"));
  const follow = q.get("follow");
  return {
    at: "machine",
    machine,
    ...(pane !== undefined ? { pane } : {}),
    ...(q.has("share") ? { share: true } : {}),
    ...(follow ? { follow } : {}),
    ...(q.has("pop") ? { pop: true } : {}),
    ...(q.has("agent") ? { agent: true } : {}),
  };
}

/** The path for `m`, after the project's (`/p/<project>`) or alone;
 * `slug` is the machine's name, for its segment. */
export function machinesPath(m: MachineAt, slug?: string | null): string {
  const q = new URLSearchParams();
  let path = "/machines";
  if (m.at === "machines") {
    if (m.pane !== undefined) q.set("pane", String(m.pane));
    if (m.name) q.set("machine", m.name);
  } else if (m.at === "swarm") {
    path += "/swarm";
    if (m.focus) q.set("focus", `${m.focus.machine ? `${m.focus.machine}.` : ""}${m.focus.pane}`);
    if (m.board) q.set("board", m.board);
  } else {
    path += `/${slug ? `${m.machine}-${slug}` : m.machine}`;
    if (m.pane !== undefined) q.set("pane", String(m.pane));
    if (m.follow) q.set("follow", m.follow);
  }
  // Flags as bare words, as the shell's fragments have them.
  const flags = m.at === "machine" ? (["share", "pop", "agent"] as const).filter((k) => m[k]) : [];
  const qs = [q.toString(), ...flags].filter(Boolean).join("&");
  const layer = m.at === "machines" && m.layer ? `#${m.layer}` : "";
  return `${path}${qs ? `?${qs}` : ""}${layer}`;
}

/** Control's page's address for `m` (its fragment, without the `#`): where
 * the app sends it while the hub can't reach machines from the app's host. */
export function controlHash(m: MachineAt): string {
  if (m.at === "swarm") {
    if (m.board) return `swarm=board:${m.board}`;
    return m.focus ? `swarm=${m.focus.machine ? `${m.focus.machine}.` : ""}${m.focus.pane}` : "swarm";
  }
  if (m.at === "machines") return m.layer ?? (m.pane !== undefined ? `pane=${m.pane}` : "");
  if (m.pop && m.pane !== undefined) return `pop=${m.machine}.${m.pane}`;
  if (m.pane !== undefined) return `pane=${m.machine}.${m.pane}${m.share ? "&share" : ""}${m.follow ? `&follow=${encodeURIComponent(m.follow)}` : ""}`;
  return `machine=${m.machine}${m.agent ? "&agent" : ""}`;
}

/** Where one of control's page addresses lives in the app, project-less
 * (the app puts it in the current project), or null when it stays on
 * control's page: a secret (`#link=`, `#join=`…), signing in, a passkey, a
 * deploy's approval, or one the platform already took (`#chat…`). Keep
 * this forever: old links live in chat, notifications and Docs (decision
 * 11). */
export function fromControl(hash: string, search = ""): string | null {
  const q = new URLSearchParams(search);
  if (hash === "" || hash === "#") {
    const name = q.get("machine");
    return name ? machinesPath({ at: "machines", name }) : "/";
  }
  let m = /^#pane=(?:([0-9a-f]+)\.)?(\d+)(?:&thread=(?:pane|session)-\d+)?(&share)?(?:&follow=([^&]+))?$/.exec(hash);
  if (m) {
    const pane = Number(m[2]);
    if (!m[1]) return machinesPath({ at: "machines", pane });
    let follow: string | undefined;
    try {
      follow = m[4] && decodeURIComponent(m[4]);
    } catch {
      follow = undefined;
    }
    return machinesPath({ at: "machine", machine: m[1], pane, ...(m[3] ? { share: true } : {}), ...(follow ? { follow } : {}) });
  }
  m = /^#machine=([0-9a-f]{6,64})(&agent)?$/.exec(hash);
  if (m) return machinesPath({ at: "machine", machine: m[1], ...(m[2] ? { agent: true } : {}) });
  m = /^#swarm(?:=(?:board:(brd_[\w-]+)|(?:([0-9a-f]+)\.)?(\d+)))?$/.exec(hash);
  if (m) return machinesPath({ at: "swarm", ...(m[1] ? { board: m[1] } : {}), ...(m[3] ? { focus: { ...(m[2] ? { machine: m[2] } : {}), pane: Number(m[3]) } } : {}) });
  m = /^#pop=(?:([0-9a-f]+)\.)?(\d+)$/.exec(hash);
  if (m) return m[1] ? machinesPath({ at: "machine", machine: m[1], pane: Number(m[2]), pop: true }) : machinesPath({ at: "machines", pane: Number(m[2]) });
  m = /^#project=(prj_[0-9a-f]{4,64})$/.exec(hash);
  if (m) return `/p/${m[1]}`;
  if (hash === "#docs") return "/docs";
  const layer = hash.slice(1);
  if (MOVES.test(layer)) return machinesPath({ at: "machines", layer });
  return null;
}
