// The session overview (M111, #1026): `#overview`, the shown machine's
// sessions, each with its tabs and panes, what they need from you and
// their branch, from `State` only. The brand and the session breadcrumb
// lead here (`#overview=<session>` brings that session to the top of the
// eye). It lies over the tabs and panes, which stay laid out under it, so
// no terminal changes size; clicking a pane goes to it.

import { useEffect, useLayoutEffect, useRef } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import { cleanTitle } from "../../../shared/titles";
import type { PaneId, PaneInfo, SessionId, TabId } from "../../../shared/proto";
import { AttentionBadge } from "../attention";
import { useSubscribe } from "../../../shared/ui/hooks";
import { escapeTaken } from "../../../shared/ui/menu";
import { closeOverview, overviewRoute } from "../places";
import { BranchIcon } from "./icons";
import { showSession, summarize } from "./session-sidebar";
import { cloudTabAttention, cloudWorkspaces, cloudWorkspaceState, useCloudPanes } from "../../../shared/cloud";
import { openPlatform, platformHref, platformOrigin, workspacePath } from "../../platform-links";

/** Whether `#overview` is up, kept current with the route. */
export function useOverview(): { session?: number } | null {
  useSubscribe((fn) => {
    addEventListener("hashchange", fn);
    return () => removeEventListener("hashchange", fn);
  });
  return overviewRoute();
}

const kindWord = (i: PaneInfo): string => {
  if (i.type === "terminal") return i.kind && i.kind !== "shell" ? i.kind : "shell";
  return i.type;
};

/** A pane by name: the title its program set, unless that's a shell's own
 * "user@host: dir" (the directory is shown beside it); else what runs;
 * else its directory's last part. The palette's jumps use it too. */
export function paneName(client: Client, i: PaneInfo): string {
  const title = cleanTitle(client.title(i.id));
  if (title && !/^[^\s@]+@[^\s:]+:/.test(title)) return title;
  const cwd = client.cwd(i.id);
  return i.current?.text || i.command || (cwd ? `shell in ${cwd.split("/").filter(Boolean).pop() ?? "/"}` : "") || `pane %${i.id}`;
}

/** "~/dev/arugula" for a home directory, and only its last two parts when
 * it's deeper ("…/worktrees/m111"); the tooltip has it whole. */
function shortCwd(cwd: string | null): string | null {
  if (!cwd) return null;
  const home = cwd.replace(/^\/(?:home|Users)\/[^/]+/, "~");
  const parts = home.split("/");
  return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : home;
}

function goToPane(client: Client, pane: PaneId) {
  closeOverview();
  client.setActive(pane);
  requestAnimationFrame(() => client.viewOf(pane)?.focus());
}

function goToTab(client: Client, tab: TabId) {
  closeOverview();
  client.selectTab(tab);
  requestAnimationFrame(() => client.viewOf(client.active() ?? -1)?.focus());
}

function goToSession(client: Client, s: SessionId) {
  showSession(client, s);
  requestAnimationFrame(() => client.viewOf(client.active() ?? -1)?.focus());
}

export function SessionOverview({ client, session }: { client: Client; session?: number }) {
  useSubscribe((fn) => directory.subscribe(fn));
  const root = useRef<HTMLElement>(null);
  const state = client.state!;
  const host = directory.current || "this machine";
  // The named session in view, else the whole page; keys start here.
  useLayoutEffect(() => {
    const el = session !== undefined ? root.current?.querySelector(`[data-overview-session="${session}"]`) : null;
    if (el) (el as HTMLElement).scrollIntoView({ block: "nearest" });
    ((el?.querySelector(".ws-ov-name") as HTMLElement | null) ?? root.current)?.focus();
  }, [session]);
  // Escape anywhere goes back to the panes (not only from inside it: a
  // click on its background leaves the focus on the body).
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || escapeTaken(e)) return;
      // A dialog or the drawer over it takes its own Escape.
      if (document.querySelector(".prompt-backdrop, [data-attention-drawer]")) return;
      e.preventDefault();
      closeOverview();
      client.viewOf(client.active() ?? -1)?.focus();
    };
    addEventListener("keydown", fn);
    return () => removeEventListener("keydown", fn);
  }, [client]);
  const rows = state.sessions.map((s) => summarize(client, s));
  const panes = rows.reduce((n, r) => n + r.panes, 0);
  const needs = state.panes.filter((p) => p.attention === "needs_input" && p.type !== "remote").length;
  const active = client.active();
  return (
    <section class="ws-overview" aria-label={`Sessions on ${host}`} data-overview tabIndex={-1} ref={root}>
      <header class="ws-ov-head">
        <div>
          <h1>Sessions on {host}</h1>
          <p class="ws-ov-sum">
            {rows.length} session{rows.length === 1 ? "" : "s"} · {panes} pane{panes === 1 ? "" : "s"}
            {needs ? ` · ${needs} need${needs === 1 ? "s" : ""} you` : ""}
          </p>
        </div>
        <button
          class="ws-ov-back"
          title="Back to the panes (Escape)"
          onClick={() => {
            closeOverview();
            client.viewOf(client.active() ?? -1)?.focus();
          }}
        >
          Back to panes
        </button>
      </header>
      {rows.length === 0 && (
        <div class="ws-empty" role="status">
          <p>No sessions on {host}.</p>
        </div>
      )}
      <div class="ws-ov-grid">
        {rows.map((row) => {
          const s = row.session;
          return (
            <article
              key={s.id}
              class={`ws-ov-session${s.id === client.session ? " current" : ""}${s.id === session ? " picked" : ""}`}
              data-overview-session={s.id}
              data-attention={row.attention}
            >
              <header class="ws-ov-session-head">
                <button class="ws-ov-name" title={`Show ${s.name} at its last tab`} onClick={() => goToSession(client, s.id)}>
                  <span class={`ws-dot ${row.attention}`} aria-hidden="true" />
                  <span class="ws-ellipsis">{s.name}</span>
                </button>
                <AttentionBadge state={row.attention} reason={row.reason} pane={row.pane} client={client} />
                <span class="ws-ov-counts">
                  {row.tabs} tab{row.tabs === 1 ? "" : "s"} · {row.panes} pane{row.panes === 1 ? "" : "s"}
                </span>
              </header>
              {row.context && (
                <p class="ws-ov-context" title={row.context}>
                  {row.context.startsWith("Multiple") ? null : <BranchIcon size={11} />}
                  <span class="ws-ellipsis">{row.context}</span>
                </p>
              )}
              {s.tabs.map((tid) => {
                const t = client.tabView(tid);
                if (!t) return null;
                return (
                  <div key={tid} class="ws-ov-tab" data-overview-tab={tid}>
                    <button class="ws-ov-tab-name" onClick={() => goToTab(client, tid)} title="Show this tab">
                      {tabLabel(client, t)}
                    </button>
                    <ul class="ws-ov-panes">
                      {paneIds(t).map((p) => {
                        const i = client.info(p);
                        if (!i) return null;
                        const cwd = shortCwd(client.cwd(p));
                        return (
                          <li key={p}>
                            <button
                              class={`ws-ov-pane${p === active && s.id === client.session ? " active" : ""}`}
                              data-overview-pane={p}
                              data-attention={i.attention}
                              title={`Go to this pane${i.attention === "needs_input" ? " (this doesn't answer it)" : ""}${client.cwd(p) ? `\n${client.cwd(p)}` : ""}`}
                              onClick={() => goToPane(client, p)}
                            >
                              <span class={`ws-dot ${i.attention}`} aria-hidden="true" />
                              <span class="ws-ov-kind">{kindWord(i)}</span>
                              <span class="ws-ov-pane-name ws-ellipsis">{paneName(client, i)}</span>
                              {(cwd || i.branch) && (
                                <span class="ws-ov-where ws-ellipsis">{[cwd, i.branch].filter(Boolean).join(" · ")}</span>
                              )}
                              <AttentionBadge state={i.attention} reason={i.reason} />
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </div>
                );
              })}
            </article>
          );
        })}
      </div>
      <CloudWorkspaceCards />
    </section>
  );
}

/** The Cloud workspaces after the machine's sessions, each with its status
 * and tabs, as links to the platform page, where they are (M153; the
 * shell draws none of its views since M155). None without one. */
function CloudWorkspaceCards() {
  useCloudPanes();
  const list = cloudWorkspaces.list;
  if (!platformOrigin() || !list.length) return null;
  const go = (e: MouseEvent, path: string) => {
    e.preventDefault();
    openPlatform(path);
  };
  return (
    <>
      <h2 class="ws-ov-cloud-head">
        Cloud workspaces <span class="dim">↗</span>
      </h2>
      <div class="ws-ov-grid">
        {list.map((w) => {
          const { word, dot } = cloudWorkspaceState(w);
          return (
            <article key={w.id} class="ws-ov-session" data-overview-cloud-workspace={w.id}>
              <header class="ws-ov-session-head">
                <a class="ws-ov-name" href={platformHref(workspacePath(w.id))} title={`${w.name}, on the platform page`} onClick={(e) => go(e, workspacePath(w.id))}>
                  <span class={`ws-dot ${dot}`} aria-hidden="true" />
                  <span class="ws-ellipsis">{w.name}</span>
                </a>
                <span class="ws-ov-counts">{word}</span>
              </header>
              <ul class="ws-ov-panes">
                {(cloudWorkspaces.tabs.get(w.id) ?? []).map((t) => (
                  <li key={t.id}>
                    <a
                      class="ws-ov-pane"
                      href={platformHref(workspacePath(w.id, t.id))}
                      data-overview-cloud-tab={t.id}
                      data-attention={cloudTabAttention(t)}
                      title={`This tab, on the platform page${cloudTabAttention(t) === "needs_input" ? " (this doesn't answer it)" : ""}\n${t.cwd}`}
                      onClick={(e) => go(e, workspacePath(w.id, t.id))}
                    >
                      <span class={`ws-dot ${cloudTabAttention(t)}`} aria-hidden="true" />
                      <span class="ws-ov-kind">{t.runtime}</span>
                      <span class="ws-ov-pane-name ws-ellipsis">{t.agent_name}</span>
                      <span class="ws-ov-where ws-ellipsis">{t.cwd}</span>
                    </a>
                  </li>
                ))}
              </ul>
            </article>
          );
        })}
      </div>
    </>
  );
}
