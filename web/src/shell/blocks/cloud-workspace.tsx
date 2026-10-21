// Cloud workspace blocks (#1117): one of the machine owner's Cloud
// workspaces beside a pane, put there by an agent (`show` kind
// `cloud_workspace`). The daemon reads the workspace and its tabs from
// control while the block is drawn (the block's state), and this draws its
// overview: its status, repositories and tabs. A tab, its transcript and
// the workspace's Changes are the platform page's (M153): since M155 the
// shell draws none of its views, and each opens there, or on control's page
// (the state's `link`) when this page doesn't know a platform page.

import { render } from "preact";
import type { Client } from "../client";
import { cloudTabAttention, cloudWorkspaceState, type CloudTab, type CloudWorkspace } from "../../shared/cloud";
import type { PaneId } from "../../shared/proto";
import { platformHref, workspacePath } from "../platform-links";
import { registerBlock, type BlockView } from "./view";

interface CloudWorkspaceBlockState {
  cloud_workspace: string;
  tab: string | null;
  workspace: CloudWorkspace | null;
  tabs: CloudTab[];
  link: string | null;
  error: string | null;
  loading: boolean;
}

const tabName = (t: CloudTab) => `${t.agent_name} · ${t.runtime}`;

/** Where the workspace (or one of its tabs) is: the platform page, else
 * control's page, whose old `#cloud-ws=` link takes you there (M154). */
function hrefOf(s: CloudWorkspaceBlockState, tab?: string | null): string | undefined {
  const there = platformHref(workspacePath(s.cloud_workspace, tab ?? undefined));
  if (there) return there;
  if (!s.link) return undefined;
  return tab ? s.link.replace(/#cloud-ws=.*$/, `#cloud-ws=${s.cloud_workspace}/${tab}`) : s.link;
}

/** A link out, in a tab of its own. */
function Out({ href, title, children, ...data }: { href: string | undefined; title: string; children: string; [k: `data-${string}`]: unknown }) {
  if (!href) return null;
  return (
    <a class="browser-open" href={href} target="_blank" rel="noopener noreferrer" title={title} {...data}>
      {children}
    </a>
  );
}

function Overview({ s, w, tabs, may }: { s: CloudWorkspaceBlockState; w: CloudWorkspace; tabs: CloudTab[]; may: boolean }) {
  const { word } = cloudWorkspaceState(w);
  const spec = w.saved_spec;
  return (
    <div class="cloud-ws-home" data-cloud-ws-block-home>
      <p class="dim" data-cloud-home-status>
        {word}
        {w.status === "provisioning" && ": installing packages, cloning repositories and running the setup script."}
        {w.status === "ready" && w.sandbox_status === "suspended" && ": its machine sleeps until a tab gets a prompt."}
      </p>
      {w.status === "failed" && w.error && <pre class="cloud-ws-error" role="alert" data-cloud-error>{w.error}</pre>}
      <h3>Repositories</h3>
      {spec.repositories.length ? (
        <ul class="cloud-ws-repos">{spec.repositories.map((r) => <li key={r.mount ?? r.url}><span class="mono">{r.mount}</span> · {r.url}{r.ref ? ` @ ${r.ref}` : ""}</li>)}</ul>
      ) : (
        <p class="dim">None: tabs start in /home/sprite.</p>
      )}
      <h3>Tabs</h3>
      {tabs.length ? (
        <ul class="cloud-ws-tabs">
          {tabs.map((t) => (
            <li key={t.id} data-cloud-tab={t.id} data-attention={cloudTabAttention(t)}>
              {may ? <Out href={hrefOf(s, t.id)} title={`${t.agent_name}'s transcript, where it's answered`} data-cloud-tab-link={t.id}>{tabName(t)}</Out> : tabName(t)}{" "}
              <span class="dim">{t.status.replaceAll("_", " ")} · {t.cwd}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p class="dim">No tabs yet.</p>
      )}
      {may ? (
        <p class="dim">
          Its tabs' transcripts, their prompts and its Changes are on the{" "}
          <Out href={hrefOf(s)} title="This Cloud workspace's changes and setup" data-cloud-ws-changes-link>
            Cloud workspace's page
          </Out>
          .
        </p>
      ) : (
        <p class="dim">You're watching this session: its tabs' transcripts and changes are for the people who can change it.</p>
      )}
    </div>
  );
}

function CloudWorkspaceBlock({ client, id, s }: { client: Client; id: PaneId; s: CloudWorkspaceBlockState | null }) {
  if (!s) return <div class="review"><p class="dim">Loading…</p></div>;
  const w = s.workspace;
  const tabs = s.tabs ?? [];
  const may = client.role(client.sessionOfPane(id)) !== "viewer";
  const state = w ? cloudWorkspaceState(w) : null;
  return (
    <div class="review cloud-ws-block" data-cloud-ws-block={id} data-cloud-workspace={s.cloud_workspace} data-cloud-state={state?.word}>
      <div class="review-bar">
        <span class="review-path">
          <b>{w?.name ?? s.cloud_workspace}</b> <span class="dim">Cloud workspace{state ? ` · ${state.word}` : ""}</span>
        </span>
        <Out href={hrefOf(s, s.tab)} title="Open this Cloud workspace where it's worked on" data-cloud-ws-link>
          Open ↗
        </Out>
      </div>
      <div class="review-body cloud-ws-view">
        {s.error && <p role="alert" class="control-error">{s.error}</p>}
        {!w ? (
          <p class="dim cloud-ws-block-note">{s.loading ? "Reading it from Arugula control…" : "Arugula control didn't say what it is."}</p>
        ) : (
          <Overview s={s} w={w} tabs={tabs} may={may} />
        )}
      </div>
    </div>
  );
}

function plain(s: CloudWorkspaceBlockState | null): string {
  if (!s) return "";
  const w = s.workspace;
  const lines = [w ? `${w.name} (${s.cloud_workspace}): ${cloudWorkspaceState(w).word}` : s.cloud_workspace];
  for (const t of s.tabs ?? []) lines.push(`${t.id} ${tabName(t)}: ${t.status}`);
  if (s.error) lines.push(s.error);
  return lines.join("\n");
}

registerBlock("cloud_workspace", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-cloud-workspace";
  let state: CloudWorkspaceBlockState | null = null;
  const draw = () => render(<CloudWorkspaceBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as CloudWorkspaceBlockState;
      draw();
    },
    title: () => state?.workspace?.name ?? "Cloud workspace",
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("a")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
});
