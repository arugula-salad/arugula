// The apps registered clients keep in a project (M236): studio's apps, in
// the project's settings, for its members. An app in a project is its
// members' to open in that product (editors and owners), so its owners are
// the project's. The product puts it here for a member who edits the
// project; nothing on this page changes it.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { nameIn } from "../pages/issues";
import { ago } from "./agent-tokens";

type App = { client: string; app: string; title: string; added_by: string | null; added_at: number };

export function ClientApps({ p }: { p: PlatformProject }) {
  const [apps, setApps] = useState<App[] | null>(null);
  useEffect(() => {
    platformFetch(`/api/projects/${encodeURIComponent(p.id)}/clients`, { credentials: "same-origin" })
      .then((r) => (r.ok ? (r.json() as Promise<{ apps: App[] }>) : null))
      .then((v) => setApps(v?.apps ?? null), () => setApps(null));
  }, [p.id]);
  if (!apps?.length) return null;
  return (
    <div data-client-apps>
      <h2>Apps from products</h2>
      <p class="dim">Apps other products keep in this project. Its editors and owners open them there; its viewers see them listed.</p>
      <ul class="platform-list">
        {apps.map((a) => (
          <li key={`${a.client}/${a.app}`} data-client-app={a.app}>
            <strong>
              <bdi>{a.title}</bdi>
            </strong>{" "}
            <span class="dim">
              {a.client} · <code>{a.app}</code> · added {ago(a.added_at)}
              {a.added_by ? (
                <>
                  {" "}
                  by <bdi>{nameIn(p)(a.added_by) ?? a.added_by}</bdi>
                </>
              ) : null}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
