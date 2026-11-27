// What the agent panel (./panel.tsx) shows in a tab, and the key a tab goes
// by: kept apart from the component so the page and its tests can name a
// tab without drawing one. Run with `node --experimental-strip-types`.

/** A tab's target: an agent's session on a machine (by the machine's id, as
 * `fleet.hostById` takes it), or a chat room, or one message's reply thread
 * in it (`parent`, its seq). */
export type PanelTarget =
  | { kind: "agent"; role: "lead" | "worker" | "agent"; machine: string; session?: string; pane?: number; board?: string; row?: string; title?: string }
  | { kind: "thread"; room: string; parent?: number; title: string };

/** `agent:<machine>:<session|%pane>` or `thread:<room>[:<parent>]`. */
export const panelKey = (t: PanelTarget): string =>
  t.kind === "agent" ? `agent:${t.machine}:${t.session ?? `%${t.pane ?? ""}`}` : `thread:${t.room}${t.parent !== undefined ? `:${t.parent}` : ""}`;

/** The chief of staff's tab, the panel's first: the key `onPick` is given for it. */
export const CHIEF_KEY = "chief";
