// The apps registered clients keep in a project (M236): studio's apps, in
// the project's settings, for its members. An app in a project is its
// members' to open in that product (editors and owners), so its owners are
// the project's. The product puts it here for a member who edits the
// project; nothing on this page changes it. A section (./section), a row
// each.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { nameIn } from "../pages/issues";
import { ago } from "./agent-tokens";
import { Section } from "./section";

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
    <Section
      id="apps"
      title="Apps from products"
      count={apps.length}
      hooks={{ "data-client-apps": "" }}
      lede="Apps other products keep in this project."
      more={
        <p>
          Its editors and owners open them in that product; its viewers see them listed. A product puts one here for a member who edits the
          project, and nothing here changes them.
        </p>
      }
    >
      <ul class="platform-list settings-rows">
        {apps.map((a) => (
          <li key={`${a.client}/${a.app}`} data-client-app={a.app}>
            <strong>
              <bdi>{a.title}</bdi>
            </strong>
            <span class="settings-tag">{a.client}</span>
            <code class="sw-mono">{a.app}</code>
            <span class="dim">
              added {ago(a.added_at)}
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
    </Section>
  );
}
