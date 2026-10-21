// `/p/:project/workspaces/:id`, and `/:tab` for one of its tabs: a Cloud
// workspace (M150): its status, repositories and tabs, and a tab's
// transcript with its approvals and questions, the prompt box and
// Interrupt (../cloud/workspaces' `CloudTabView`).

import { cloudTabAttention, cloudWorkspaces, cloudWorkspaceState } from "../../shared/cloud";
import { useSubscribe } from "../../shared/ui/hooks";
import { CloudTabView } from "../cloud/workspaces";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { Missing } from "./project";

export function WorkspacePage({ project, id, tab }: { project: string; id: string; tab?: string }) {
  useSubscribe((fn) => cloudWorkspaces.subscribe(fn));
  const w = cloudWorkspaces.find(id);
  if (!w) return cloudWorkspaces.loaded ? <Missing what="That Cloud workspace isn't one of yours (or it's gone)." /> : <p class="platform-section dim">Loading…</p>;
  const tabs = cloudWorkspaces.tabs.get(w.id) ?? [];
  const named = (x: string) => (x === w.id ? w.name : names(x));
  const at = (t?: string) => pathOf({ page: "workspace", project, workspace: w.id, tab: t }, named);
  const { word } = cloudWorkspaceState(w);
  return (
    <section class="platform-workspace" data-platform-workspace={w.id}>
      <div class="tabstrip cloud-tabstrip">
        <div class="tabbar" role="tablist" aria-label={`Tabs in ${w.name}`}>
          <a class={tab ? "tab" : "tab selected"} role="tab" aria-selected={!tab} href={at()} data-cloud-ws-overview onClick={(e) => follow(e, at())}>
            <span class="tab-label">{w.name}</span>
          </a>
          {tabs.map((t) => {
            const attention = cloudTabAttention(t);
            return (
              <a key={t.id} class={tab === t.id ? "tab selected" : "tab"} role="tab" aria-selected={tab === t.id} href={at(t.id)} data-cloud-tab={t.id} data-attention={attention} onClick={(e) => follow(e, at(t.id))}>
                <span class="host-tag">{t.runtime}</span>
                <span class="tab-label">{t.agent_name}</span>
                {attention === "needs_input" ? <span class="attention" title="Needs you">●</span> : attention === "working" ? <span class="attention working" title="Working">●</span> : null}
              </a>
            );
          })}
        </div>
      </div>
      <div class="cloud-ws-view">
        {tab ? (
          <CloudTabView key={tab} w={w} tab={tabs.find((t) => t.id === tab)} id={tab} tabs={tabs} />
        ) : (
          <div class="cloud-ws-home" data-cloud-ws-home>
            <p class="dim" data-cloud-home-status>
              {word}
            </p>
            {w.status === "failed" && w.error && <pre class="cloud-ws-error" role="alert">{w.error}</pre>}
            <h3>Repositories</h3>
            {w.saved_spec.repositories.length ? (
              <ul class="cloud-ws-repos">
                {w.saved_spec.repositories.map((r) => (
                  <li key={r.mount ?? r.url}>
                    <span class="mono">{r.mount}</span> · {r.url}
                    {r.ref ? ` @ ${r.ref}` : ""}
                  </li>
                ))}
              </ul>
            ) : (
              <p class="dim">None: tabs start in /home/sprite.</p>
            )}
            <h3>Tabs</h3>
            {tabs.length ? (
              <ul class="cloud-ws-tabs">
                {tabs.map((t) => (
                  <li key={t.id}>
                    <a href={at(t.id)} onClick={(e) => follow(e, at(t.id))}>
                      {t.agent_name} · {t.runtime}
                    </a>{" "}
                    <span class="dim">
                      {t.status.replaceAll("_", " ")} · {t.cwd}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p class="dim">No tabs yet.</p>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
