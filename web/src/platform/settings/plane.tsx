// A project's Plane connection (Arugula #356), a section of its settings
// after the linked repos: the Plane projects whose work items live in this
// project, what agents may write to each, and a form to connect another.
// Connecting takes a Plane API key, which control reads the project with
// and seals; it is never shown again, so the field is a password input and
// is cleared after the form is sent. A team's viewers see the list only.

import { useEffect, useState } from "preact/hooks";
import type { TrackerConnection, TrackerConnections } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import type { PlatformProject } from "../session";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { Section } from "./section";

/** A call to the platform: its JSON, or its `{error}` thrown. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

const hours = (ms: number) => Math.max(1, Math.ceil((ms - Date.now()) / 3_600_000));

export function PlaneConnections({ p }: { p: PlatformProject }) {
  const [conns, setConns] = useState<TrackerConnection[] | null>(null);
  const [workspace, setWorkspace] = useState("");
  const [project, setProject] = useState("");
  const [key, setKey] = useState("");
  const [hook, setHook] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [disconnecting, setDisconnecting] = useState<TrackerConnection | null>(null);
  const base = `/api/projects/${encodeURIComponent(p.id)}/trackers`;
  const changes = !p.team || p.team.role !== "viewer";
  const load = () =>
    call<TrackerConnections>("GET", base).then(
      (v) => setConns(v.connections),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [p.id]);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr("");
    try {
      await f();
      await load();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const submit = (e: Event) => {
    e.preventDefault();
    const body = { workspace_slug: workspace.trim(), project: project.trim(), api_key: key.trim(), webhook_secret: hook.trim() || undefined };
    // The key leaves the form as it's sent, whatever the answer.
    setKey("");
    setHook("");
    void act(async () => {
      await call("POST", base, body);
      setProject("");
    });
  };
  const connected = (conns ?? []).filter((c) => c.purge_after === null).length;
  const waiting = (conns ?? []).reduce((n, c) => n + (c.purge_after === null ? c.waiting_writes : 0), 0);
  const triage = pathOf({ page: "triage", project: p.id }, names);
  return (
    <Section
      id="plane"
      title="Plane"
      count={conns ? connected : undefined}
      lede="Plane projects whose work items live in this project, for its members."
      hooks={{ "data-plane": p.id }}
      more={
        <>
          <p>
            Connecting takes a Plane API key from the person who connects it. Control reads the project with it, keeps the key sealed, and never shows it again. Disconnecting stops
            reading the project at once and removes the key; what was read of it goes within a day.
          </p>
          <p>The webhook secret is optional: it is what Plane signs live updates with.</p>
        </>
      }
    >
      {waiting > 0 && (
        <p data-plane-waiting>
          {waiting} agent {waiting === 1 ? "write is" : "writes are"} waiting for approval in{" "}
          <a href={triage} onClick={(e) => follow(e, triage)}>
            Triage
          </a>
          .
        </p>
      )}
      <div data-plane-connections>
        {conns?.length === 0 && (
          <p class="dim" data-plane-none>
            No Plane project is connected. {changes ? "Connect one below." : ""}
          </p>
        )}
        {conns?.length ? (
          <table class="settings-table">
            <thead>
              <tr>
                <th scope="col">Project</th>
                <th scope="col">Items</th>
                <th scope="col">Agent writes</th>
                <th scope="col">
                  <span class="settings-hidden">Disconnect</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {conns.map((c) => (
                <tr key={c.id} data-plane-connection={c.id}>
                  <th scope="row">
                    <span class="repo-name">{c.identifier}</span> {c.name} <span class="dim">{c.workspace_slug}</span>
                  </th>
                  <td data-col="Items">{c.purge_after !== null ? <span class="dim">disconnected, gone within {hours(c.purge_after)}h</span> : c.backfilled_at === null ? <span class="dim">{c.items} so far…</span> : c.items}</td>
                  <td data-col="Agent writes">
                    {c.purge_after === null && (
                      <select
                        aria-label={`What agents may write to ${c.identifier}`}
                        data-plane-writes={c.id}
                        value={c.agent_writes}
                        disabled={busy || !changes}
                        onChange={(e) => {
                          const agent_writes = e.currentTarget.value;
                          void act(() => call("PATCH", `${base}/${c.id}`, { agent_writes }));
                        }}
                      >
                        <option value="approve">Approve each write</option>
                        <option value="allow">Allow all writes</option>
                      </select>
                    )}
                  </td>
                  <td class="settings-row-act">
                    {changes && c.purge_after === null && (
                      <button class="secondary danger small" data-plane-disconnect={c.id} disabled={busy} onClick={() => setDisconnecting(c)}>
                        Disconnect
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : null}
      </div>
      {changes ? (
        <form class="platform-link-repo" aria-label="Connect a Plane project" data-plane-form onSubmit={submit}>
          <h3>Connect a Plane project</h3>
          <input required aria-label="Workspace" placeholder="Workspace slug" autocomplete="off" value={workspace} data-plane-workspace onInput={(e) => setWorkspace(e.currentTarget.value)} />
          <input required aria-label="Project" placeholder="Project id or identifier" autocomplete="off" value={project} data-plane-project onInput={(e) => setProject(e.currentTarget.value)} />
          <input required type="password" autocomplete="off" aria-label="API key" placeholder="Plane API key" value={key} data-plane-key onInput={(e) => setKey(e.currentTarget.value)} />
          <input type="password" autocomplete="off" aria-label="Webhook secret (optional)" placeholder="Webhook secret (optional)" value={hook} data-plane-hook onInput={(e) => setHook(e.currentTarget.value)} />
          <button class="primary" disabled={busy}>
            Connect
          </button>
        </form>
      ) : (
        <p class="dim">A team's owners and editors connect Plane projects.</p>
      )}
      {err && (
        <p class="control-error" data-plane-error>
          {err}
        </p>
      )}
      {disconnecting && (
        <ConfirmRemove
          title={`Disconnect ${disconnecting.identifier}?`}
          label="Disconnect"
          cancel={() => setDisconnecting(null)}
          go={() => {
            const c = disconnecting;
            setDisconnecting(null);
            void act(() => call("DELETE", `${base}/${c.id}`));
          }}
        >
          <p>The project stops reading it at once and forgets its key, and its work items go within a day.</p>
        </ConfirmRemove>
      )}
    </Section>
  );
}
