// Attention that belongs to no pane (#551): a card for what the machine
// wants its owner for and no pane can show. Today that is a teammate's
// mention that no session took. The drawer, the bell and the phone's sheet
// draw them from the `State` (and `Delta`s) each machine sends its owner.

import type { Notice } from "./proto";

/** A card's title: what it says. A mention counts up ("2 mentions in
 * #agents"); one alone says who, where and the first line. */
export function noticeTitle(n: Notice): string {
  if (n.kind === "grant_ask") {
    const room = (n.room_name ?? n.room ?? "a room").replace(/^#/, "");
    const said = n.count > 1 ? `${n.count} mentions of ${n.grant?.machine_name ?? "this machine"} in #${room}.` : n.headline;
    return `${said} ${grantQuestion(n)}`;
  }
  if (n.kind === "mention" && n.count > 1) return `${n.count} mentions in #${(n.room_name ?? n.room ?? "a room").replace(/^#/, "")}`;
  return n.headline;
}

/** What the card's primary button says. */
export const noticeOpenLabel = (n: Notice): string => (n.kind === "mention" ? "Open chat" : "Open");

/** The room a card opens, if it opens one. A grant card asks first: it
 * opens nothing. */
export const noticeRoom = (n: Notice): string | null => (n.kind === "mention" ? (n.room ?? null) : null);

/** What Grant would give, said plainly (#578). It is `threads.read` over the
 * grant's scope, not over one room: a team's project reaches all its
 * channels and issues, "account" all the owner's DMs. With no scope name the
 * grant is just the room. */
export function grantQuestion(n: Notice): string {
  const machine = n.grant?.machine_name || "this machine";
  const room = (n.room_name ?? n.room ?? "the room").replace(/^#/, "");
  const g = n.grant;
  if (g?.scope_kind === "account") return `Let ${machine}'s agents read all your DMs (including #${room})?`;
  if (g?.scope_kind === "team") return `Let ${machine}'s agents read ${g.scope_name || "the team"}'s channels (including #${room})?`;
  if (g?.scope_name) return `Let ${machine}'s agents read ${g.scope_name}'s channels and issues (including #${room})?`;
  return `Let ${machine}'s agents read #${room}?`;
}

/** The scope a card's grant is over, as `POST /api/grants` takes it: an
 * account's DMs, a team's channels (Arugula #894) or a project's rooms.
 * A kind this page doesn't know grants nothing, rather than the wrong thing. */
export function grantScope(kind: string, id: string): { kind: string; id?: string } | null {
  if (kind === "account") return { kind: "account" };
  if (kind === "team" || kind === "project") return { kind, id };
  return null;
}

/** What a grant card says under its title (#578): "Never" can be undone. */
export const GRANT_HELP = "Not now keeps the mention for a later grant. Never stops asking about this room; granting from Settings undoes it.";

/** Oldest first, a key to break a tie: the order they arrive in, so a new
 * card goes to the end and the ones above it don't jump. */
export function sortNotices<T extends { n: Notice }>(items: readonly T[]): T[] {
  return [...items].sort((a, b) => a.n.since_ms - b.n.since_ms || (a.n.key < b.n.key ? -1 : a.n.key > b.n.key ? 1 : 0));
}
