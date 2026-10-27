// What's new (Discovery 2): the headlines of the releases since the last
// one shown to this person, from the headline sections of the release
// notes, built in (scripts/whats-new.mjs, web/vite.config.ts). The last
// version shown is the pref `whats-new` in the seen store (./seen): per
// account through control, else per browser. This is what picks; the
// shell draws the panel (shell/ui/whats-new.tsx).

/** Who may use an item: signed in on control's page, on a team, on the
 * hosted workspaces' list, or a Labs flag on for the machine shown. */
export type Need = "control" | "team" | "hosted" | `flag:${string}`;

export interface WhatsNewItem {
  /** The feature's name for good. */
  id: string;
  title: string;
  /** One sentence. */
  line: string;
  /** What *Try it* opens: a command of the shell's `COMMANDS`, or a route:
   * the shell's (`#swarm`) or a path on the platform page (`/chat`). */
  try?: { command: string } | { route: string };
  /** All of them, or the item is left out. */
  needs?: Need[];
}

export interface WhatsNewRelease {
  version: string;
  items: WhatsNewItem[];
}

/** The seen store's key: the last version What's new showed. */
export const PREF = "whats-new";
/** At most this many releases at once (a first run, or a long gap). */
export const MAX_RELEASES = 3;

/** A release's whole notes. */
export const notesUrl = (version: string) => `https://github.com/arugula-salad/arugula/releases/tag/v${version}`;

const parts = (v: string) => (/^\d+\.\d+\.\d+$/.test(v) ? v.split(".").map(Number) : null);

/** `a` against `b`: below 0 when older, above when newer. Not a version
 * counts as older than any. */
export function compareVersions(a: string | null, b: string | null): number {
  const [x, y] = [a ? parts(a) : null, b ? parts(b) : null];
  if (!x || !y) return x ? 1 : y ? -1 : 0;
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}

/** This build's releases, newest first: none newer than `built`. */
export function upTo(releases: WhatsNewRelease[], built: string | null): WhatsNewRelease[] {
  if (!built) return [];
  return releases.filter((r) => compareVersions(r.version, built) <= 0).slice(0, MAX_RELEASES);
}

/** What opens by itself: the releases after `last` (the pref, whatever
 * was stored), up to `built`. Nothing shown yet: the newest few. */
export function unshown(releases: WhatsNewRelease[], built: string | null, last: unknown): WhatsNewRelease[] {
  const since = typeof last === "string" && parts(last) ? last : null;
  return upTo(releases, built).filter((r) => compareVersions(r.version, since) > 0);
}

/** Only the items this person can use, and only releases left with some. */
export function usable(releases: WhatsNewRelease[], can: (need: Need) => boolean): WhatsNewRelease[] {
  return releases
    .map((r) => ({ ...r, items: r.items.filter((it) => (it.needs ?? []).every(can)) }))
    .filter((r) => r.items.length);
}

/** Whether `built` moves the pref on (never back: a page from an older
 * build, a daemon not yet updated, leaves it). */
export const advances = (built: string | null, last: unknown) => !!built && compareVersions(built, typeof last === "string" ? last : null) > 0;
