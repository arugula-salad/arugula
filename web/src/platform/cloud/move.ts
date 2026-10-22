// Moving a beta Cloud workspace off Managoat (M189): what the platform
// answers (`/api/cloud/workspaces/{id}/move`), and how the page says it.
// No JSX here, so scripts/move.test.mjs reads it.

export type Change = { path: string; index: string; worktree: string };
/** A repo's work the move leaves behind on the Managoat machine. */
export type Leftover = {
  mount: string; url: string; branch: string | null; changes: Change[]; uncommitted: boolean;
  unpushed: boolean | null; compared_to: string | null; truncated: boolean; error: string | null;
};

/** The day as people say it: "Thursday, November 5". `day` is
 * `YYYY-MM-DD`, read as that day where the reader is. */
export const deletionDay = (day: string, locale?: string) =>
  new Date(`${day}T12:00:00`).toLocaleDateString(locale, { weekday: "long", month: "long", day: "numeric" });

/** The repos with work left behind. */
export const leftBehind = (repos: Leftover[]) => repos.filter((r) => r.uncommitted || r.unpushed || r.changes.length > 0);

/** What a repo leaves behind, in words. */
export function leftWords(r: Leftover): string {
  const n = r.changes.length;
  const said = [
    n ? `${n} changed or untracked file${n === 1 ? "" : "s"}` : r.uncommitted ? "uncommitted changes" : "",
    r.unpushed ? `commits not on ${r.compared_to ?? "its upstream"}` : "",
    r.unpushed === null ? "no upstream to compare commits with" : "",
  ].filter(Boolean).join(" · ") || "Nothing left behind.";
  return r.truncated ? `${said} More than this; the download has what was read.` : said;
}
