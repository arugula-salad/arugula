// A project's agent sessions in numbers (M265), on its home page: of the
// sessions `@arugula` started in the last 30 days, how many led to a pull
// request (its asker sent it, or someone opened one from its branch), how
// many of those merged, and the median time from the `@` to the PR
// (`GET /api/projects/:id/sessions/stats`).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../shared/maintenance";
import type { SessionStats } from "../shared/proto";

/** "40 s", "14 min", "3 h", "2 days". */
export function took(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h`;
  return `${Math.round(h / 24)} days`;
}

export function SessionNumbers({ project }: { project: string }) {
  const [s, setS] = useState<SessionStats | null>(null);
  useEffect(() => {
    let live = true;
    void platformFetch(`/api/projects/${encodeURIComponent(project)}/sessions/stats`, { credentials: "same-origin" })
      .then((r) => (r.ok ? (r.json() as Promise<SessionStats>) : null))
      .then((v) => live && setS(v))
      .catch(() => {});
    return () => void (live = false);
  }, [project]);
  if (!s) return null;
  const cell = (n: number, label: string, key: string) => (
    <div class="session-stat" data-session-stat={key}>
      <b>{n}</b>
      <span>{label}</span>
    </div>
  );
  return (
    <section class="session-stats" data-session-stats>
      <h3>Agent sessions, last {s.days} days</h3>
      <div class="session-stat-row">
        {cell(s.sessions, s.sessions === 1 ? "session" : "sessions", "sessions")}
        <span class="session-stat-to" aria-hidden="true">
          →
        </span>
        {cell(s.prs_opened, s.prs_opened === 1 ? "PR opened" : "PRs opened", "opened")}
        <span class="session-stat-to" aria-hidden="true">
          →
        </span>
        {cell(s.prs_merged, "merged", "merged")}
      </div>
      <p class="dim" data-session-stat="median">
        {s.median_to_pr_ms === null
          ? s.sessions
            ? "None has led to a PR yet."
            : "@arugula in one of the project's threads starts one."
          : `Median from @arugula to its PR: ${took(s.median_to_pr_ms)}.`}
      </p>
    </section>
  );
}
