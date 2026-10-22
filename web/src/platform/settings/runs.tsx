// A project's standing grant for runs (M211), in its settings: whether the
// platform may start and instruct runs on the project's hosted workspaces
// with every device asleep, signed by which device and when, and what was
// revoked. Signing and revoking are trust, done by a device ("The platform
// page is on app.arugula.io, not on the keys' origin"), so they're on
// control's page (here or on the phone), and this links there.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { controlOrigin } from "../session";

interface Grant {
  id: string;
  made: number;
  expires: number | null;
  by: string;
}

interface View {
  role: "owner" | "editor" | "viewer";
  grant: { grant: Grant; created_at: number } | null;
  past: { grant: Grant; revoke: { running: "finish" | "stop"; at: number } | null; revoked_at: number | null; superseded_at: number | null }[];
  stale: boolean;
  renew: boolean;
  workspaces: number;
  offer: boolean;
}

const day = (ms: number) => new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

export function RunGrant({ project }: { project: string }) {
  const [v, setV] = useState<View | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    platformFetch(`/api/projects/${encodeURIComponent(project)}/runs/grant`, { credentials: "same-origin" })
      .then(async (r) => {
        const j = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error((j as { error?: string }).error ?? `Request failed (${r.status})`);
        setV(j as View);
      })
      .catch((e: Error) => setErr(e.message));
  }, [project]);
  if (err) return <p class="control-error">{err}</p>;
  if (!v || (!v.grant && !v.workspaces && !v.past.length)) return null;
  const g = v.grant?.grant;
  const sign = (
    <a href={controlOrigin()} data-run-grant-sign>
      control's page
    </a>
  );
  return (
    <div data-run-grant={g ? "yes" : v.offer ? "offered" : "no"}>
      <h2>Runs</h2>
      {g ? (
        <p>
          A standing grant is in place: the platform may start runs and instruct them on every hosted workspace of this project, running
          whatever command the project's runs start, with every device asleep, {g.expires ? `until ${day(g.expires)}` : "until it's revoked"} unless
          it's revoked first. A project owner's device signed it on {day(g.made)}. Each workspace checks that signature before it starts
          anything.
          {v.stale ? " It's for an earlier delegation, so workspaces enrolled since refuse it: sign it again." : ""}
          {!v.stale && v.renew ? " It expires soon: renew it to keep runs starting." : ""}
        </p>
      ) : (
        <p class="dim">No runs start here: nobody has allowed them, or the last grant expired or was revoked.</p>
      )}
      {v.role === "owner" ? (
        <p class="dim">
          {g ? (v.stale || v.renew ? "Sign it again, or revoke it," : "Revoke it") : v.offer ? "This project has hosted workspaces. Allow runs on them" : "Allow runs"} on{" "}
          {sign}, under Devices and machines, on a computer or your phone: your devices sign it there. Deploys still ask you every time.
        </p>
      ) : (
        <p class="dim">A project owner allows runs, or revokes them, from a device.</p>
      )}
      {v.past.length ? (
        <ul class="platform-list dim" data-run-grants-past>
          {v.past.map((p) => (
            <li key={p.grant.id}>
              Allowed {day(p.grant.made)},{" "}
              {p.revoked_at ? `revoked ${day(p.revoked_at)}${p.revoke?.running === "stop" ? ", and the runs going stopped" : ""}` : p.superseded_at ? `replaced ${day(p.superseded_at)}` : "expired"}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
