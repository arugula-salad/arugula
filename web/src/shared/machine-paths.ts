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
   * machine shown (`agents`, `settings=machine`…). `pop`: the pane alone
   * in its window, on the home machine, which this address can't name
   * (#1032). Without either, the Machines page (Arugula #1346): every
   * machine's sessions in the project; `agents`, only the agents'; `show`,
   * scrolled to a machine (its id, or a name where it has none) and
   * `session`, to that session's card (Arugula #1349). */
  | { at: "machines"; pane?: number; name?: string; layer?: string; pop?: true; agents?: true; show?: string; session?: number }
  /** A machine's page, with a pane to show: shared from (`share`), a person
   * to follow there (`follow`), alone in its window (`pop`); or with
   * Start an agent open (`agent`). */
  | { at: "machine"; machine: string; pane?: number; share?: true; follow?: string; pop?: true; agent?: true }
  /** Every pane on every machine (decision 3: under Machines), at a pane's
   * card (`focus`) or on one board's agents. */
  | { at: "swarm"; focus?: { machine?: string; pane: number }; board?: string }
  /** Build's old home (Arugula #1083), every machine's panes: now the
   * Machines page's old address, which the app sends there (#1346). */
  | { at: "panes"; machine?: string; agents?: true };

/** A machine's id at the front of a path segment, before its slug: a
 * daemon's id is hex. */
const MACHINE = /^([0-9a-f]{6,64})(?:-.*)?$/;
const BOARD = /^brd_[\w-]+$/;
/** A machine's name, as a host is called. */
const NAME = /^[\w.-]{1,253}$/;
/** The shell's own places over a machine, as their fragments say them. */
const LAYER = /^(?:agents(?:=team)?|settings(?:=[\w-]+)?|developer|getting-started=\w+)$/;
/** Those of them control's page sends on to the app: the machine's own,
 * its Settings sections among them, which the machine's page there sends on
 * to Settings' This machine (Arugula #1110). The rest of Settings is the
 * app's `/settings` (shell/fragments.ts) but for Phones and browsers;
 * Getting started stays with control's sign-in and passkeys (joining a
 * machine to control). */
const MOVES = /^(?:agents(?:=team)?|settings=(?:machine|remote-sessions)|developer)$/;

const num = (v: string | null) => (v !== null && /^\d{1,9}$/.test(v) ? Number(v) : undefined);

/** The place `rest` names (the path's segments after `machines`), or null. */
export function parseMachines(rest: string[], q: URLSearchParams, hash = ""): MachineAt | null {
  if (rest.length === 0) {
    const pane = num(q.get("pane"));
    const name = q.get("machine");
    const layer = hash.replace(/^#/, "");
    const show = pane === undefined && !name ? q.get("show") : null;
    const session = show && NAME.test(show) ? num(q.get("session")) : undefined;
    return {
      at: "machines",
      ...(pane !== undefined ? { pane } : {}),
      ...(name ? { name } : {}),
      ...(LAYER.test(layer) ? { layer } : {}),
      ...(pane !== undefined && q.has("pop") ? { pop: true } : {}),
      ...(pane === undefined && !name && q.has("agents") ? { agents: true } : {}),
      ...(show && NAME.test(show) ? { show } : {}),
      ...(session !== undefined ? { session } : {}),
    };
  }
  if (rest.length !== 1) return null;
  if (rest[0] === "swarm") {
    const f = /^(?:([0-9a-f]+)\.)?(\d{1,9})$/.exec(q.get("focus") ?? "");
    const board = q.get("board");
    return { at: "swarm", ...(f ? { focus: { ...(f[1] ? { machine: f[1] } : {}), pane: Number(f[2]) } } : {}), ...(board && BOARD.test(board) ? { board } : {}) };
  }
  if (rest[0] === "panes") {
    // A machine's id, or a name where it has none (a daemon's own hosts).
    const machine = q.get("machine");
    return { at: "panes", ...(machine && NAME.test(machine) ? { machine } : {}), ...(q.has("agents") ? { agents: true } : {}) };
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
    if (m.show) q.set("show", m.show);
    if (m.show && m.session !== undefined) q.set("session", String(m.session));
  } else if (m.at === "swarm") {
    path += "/swarm";
    if (m.focus) q.set("focus", `${m.focus.machine ? `${m.focus.machine}.` : ""}${m.focus.pane}`);
    if (m.board) q.set("board", m.board);
  } else if (m.at === "panes") {
    path += "/panes";
    if (m.machine) q.set("machine", m.machine);
  } else {
    path += `/${slug ? `${m.machine}-${slug}` : m.machine}`;
    if (m.pane !== undefined) q.set("pane", String(m.pane));
    if (m.follow) q.set("follow", m.follow);
  }
  // Flags as bare words, as the shell's fragments have them.
  const flags = m.at === "machine" ? (["share", "pop", "agent"] as const).filter((k) => m[k]) : m.at === "machines" && m.pop ? ["pop"] : m.at === "machines" && m.agents ? ["agents"] : m.at === "panes" && m.agents ? ["agents"] : [];
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
  // Control's page has no list of every machine's panes: a machine's, or its own.
  if (m.at === "panes") return m.machine ? `machine=${m.machine}` : "";
  if (m.at === "machines") return m.layer ?? (m.pane !== undefined ? `${m.pop ? "pop" : "pane"}=${m.pane}` : m.show && /^[0-9a-f]{6,64}$/.test(m.show) ? `machine=${m.show}` : "");
  if (m.pop && m.pane !== undefined) return `pop=${m.machine}.${m.pane}`;
  if (m.pane !== undefined) return `pane=${m.machine}.${m.pane}${m.share ? "&share" : ""}${m.follow ? `&follow=${encodeURIComponent(m.follow)}` : ""}`;
  return `machine=${m.machine}${m.agent ? "&agent" : ""}`;
}

/** The address that opens `pane` alone in a window of its own (#497),
 * `daemon` naming its machine (none: the home machine). In the app it's the
 * machine's page with `pop`; on control's page, the fragment `#pop=`. */
export function popAddress(pane: number, daemon?: string, app = true): string {
  if (!app) return `/#pop=${daemon ? `${daemon}.` : ""}${pane}`;
  return machinesPath(daemon ? { at: "machine", machine: daemon, pane, pop: true } : { at: "machines", pane, pop: true });
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
  if (m) return popAddress(Number(m[2]), m[1]);
  m = /^#project=(prj_[0-9a-f]{4,64})$/.exec(hash);
  if (m) return `/p/${m[1]}`;
  if (hash === "#docs") return "/docs";
  // A machine's sessions over its panes, retired for the Machines page
  // (Arugula #1349): the link names no machine, so the page's top, where
  // this computer or the one you used last leads.
  if (/^#overview(?:=\d+)?$/.test(hash)) return machinesPath({ at: "machines" });
  const layer = hash.slice(1);
  if (MOVES.test(layer)) return machinesPath({ at: "machines", layer });
  return null;
}

/** Control's Settings sections that are a machine's own: This machine's. */
export const MACHINE_SETTINGS = new Set(["machine", "remote-sessions"]);

/** Where one of control's Settings sections (`#settings=<id>`, Arugula
 * #695) is in the app's Settings now (Arugula #1110): You's by the same
 * id, and a machine's own in This machine, at `machine` (the one shown;
 * without it, the page's first). Phones and browsers (`push`, and This
 * device and All your devices before it) stays on control's page, where a
 * device subscribes with its own keys: null. Any other (Help, before
 * #1108) is Settings, at You. */
export function settingsPath(section?: string, machine?: string): string | null {
  if (section === "push" || section === "device" || section === "all-devices") return null;
  if (section && MACHINE_SETTINGS.has(section)) {
    // Who reached it through Arugula cloud is part of Access and sharing.
    const sub = section === "machine" ? "" : "access";
    return machine ? `/settings/machine/${machine}${sub && `/${sub}`}` : `/settings/machine${sub && `#${sub}`}`;
  }
  // Preferences is part of Appearance (Arugula #1318), and Account is
  // Sign-in and security (Arugula #1311).
  if (section === "preferences") return "/settings/you/appearance";
  if (section === "account") return "/settings/you/security";
  return section === "appearance" ? `/settings/you/${section}` : "/settings";
}
