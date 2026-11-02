// A project's Plane connection (Arugula #356), a section of its settings
// after the linked repos: the Plane projects whose work items live in this
// project, what agents may write to each, and a form to connect another.
// Connecting takes a Plane API key, which control reads the project with
// and seals; it is never shown again, so the field is a password input and
// is cleared after the form is sent. A connection's Webhook cell (#417) shows
// the URL to give Plane and takes the webhook's secret after the connection
// exists, as Plane makes the secret only once it has the URL; the secret is
// never shown back. A team's viewers see the list only. Under the table each
// connected project has its recent changes (#441): what Arugula moved or
// wrote in Plane, and the moves to Done that failed for good, with a Retry.

import { useEffect, useState } from "preact/hooks";
import type { TrackerConnection, TrackerConnections, TrackerEvent, TrackerHistory } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { ago } from "../../shared/time";
import { ConfirmRemove } from "../../shared/ui/confirm";
import type { PlatformProject } from "../session";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { ATTRIBUTIONS, CUSTOM, KINDS, PRESETS, attributionHelp, presetOf } from "./plane-policy";
import { useBoards } from "../pages/boards";
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

/** What a connection's webhook is doing, in a line. */
export function hookStatus(c: Pick<TrackerConnection, "has_webhook_secret" | "last_delivery_at">, now = Date.now()): string {
  if (!c.has_webhook_secret) return "No secret: changes arrive within 5 minutes.";
  if (c.last_delivery_at === null) return "Waiting for a first delivery.";
  return `Last delivery ${ago(c.last_delivery_at, now, "long")}.`;
}

/** A connection's webhook: the URL to give Plane, whether a secret is kept
 * (never the secret), and a field to set, replace or remove it. */
function WebhookCell({ c, changes, busy, save }: { c: TrackerConnection; changes: boolean; busy: boolean; save: (secret: string) => Promise<void> }) {
  const [editing, setEditing] = useState(false);
  const [secret, setSecret] = useState("");
  const [copied, setCopied] = useState(false);
  const copy = () =>
    void navigator.clipboard.writeText(c.webhook_url).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }, () => {});
  const done = (e: Event) => {
    e.preventDefault();
    const v = secret.trim();
    // The secret leaves the field as it's sent, whatever the answer.
    setSecret("");
    setEditing(false);
    void save(v);
  };
  return (
    <div class="plane-hook" data-plane-webhook={c.id}>
      <span class="plane-hook-url">
        <code data-plane-hook-url>{c.webhook_url}</code>
        <button type="button" class="secondary small" data-plane-hook-copy aria-label={`Copy the webhook URL for ${c.identifier}`} onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </button>
      </span>
      <span class="plane-hook-secret">
        Secret: <b data-plane-hook-state>{c.has_webhook_secret ? "set" : "not set"}</b>
        {changes && !editing && (
          <>
            <button type="button" class="secondary small" data-plane-hook-edit disabled={busy} onClick={() => setEditing(true)}>
              {c.has_webhook_secret ? "Replace" : "Set"}
            </button>
            {c.has_webhook_secret && (
              <button type="button" class="secondary danger small" data-plane-hook-remove disabled={busy} onClick={() => void save("")}>
                Remove
              </button>
            )}
          </>
        )}
      </span>
      {editing && (
        <form class="plane-hook-form" data-plane-hook-form onSubmit={done}>
          <input
            class="field"
            required
            type="password"
            autocomplete="off"
            aria-label={`Webhook secret for ${c.identifier}`}
            value={secret}
            data-plane-hook-secret
            onInput={(e) => setSecret(e.currentTarget.value)}
          />
          <button class="primary small" disabled={busy}>
            Save
          </button>
          <button type="button" class="secondary small" onClick={() => (setSecret(""), setEditing(false))}>
            Cancel
          </button>
        </form>
      )}
      <span class="dim" data-plane-hook-status>
        {hookStatus(c)}
      </span>
    </div>
  );
}

/** How many epics a connection's boards follow. */
export function epicsLine(n: number): string {
  return n === 0 ? "No epics yet" : `Following ${n} ${n === 1 ? "epic" : "epics"}`;
}

/** One entry of a connection's history: what happened, when, and who decided it. */
export function eventLine(e: Pick<TrackerEvent, "line" | "at" | "by">, now = Date.now()): string {
  return `${e.line} · ${ago(e.at, now)}${e.by ? ` by ${e.by}` : ""}`;
}

/** A connection's recent changes: failed moves first, as errors with a Retry for those who change things, then the history. */
function RecentChanges({ c, history, changes, busy, retry }: { c: TrackerConnection; history: TrackerHistory | undefined; changes: boolean; busy: boolean; retry: (job: string) => void }) {
  const failed = history?.entries.filter((e) => e.kind === "move_failed") ?? [];
  const rest = history?.entries.filter((e) => e.kind !== "move_failed") ?? [];
  return (
    <section class="plane-history" aria-label={`${c.identifier}: recent changes`} data-plane-history={c.id}>
      <h3>{c.identifier}: recent changes</h3>
      {failed.map((e) => (
        <p class="control-error plane-failed" data-plane-failed-move={e.job ?? undefined}>
          <span>
            {e.line}
            {e.error ? `: ${e.error}` : ""}
          </span>
          {changes && e.job && (
            <button type="button" class="secondary small" data-plane-retry={e.job} disabled={busy} onClick={() => retry(e.job!)}>
              Retry
            </button>
          )}
        </p>
      ))}
      {rest.length > 0 && (
        <ul class="plane-events">
          {rest.map((e) => (
            <li data-plane-event={e.kind}>{eventLine(e)}</li>
          ))}
        </ul>
      )}
      {history && failed.length === 0 && rest.length === 0 && (
        <p class="dim" data-plane-history-none>
          Nothing yet
        </p>
      )}
    </section>
  );
}

export function PlaneConnections({ p }: { p: PlatformProject }) {
  const [conns, setConns] = useState<TrackerConnection[] | null>(null);
  const [workspace, setWorkspace] = useState("");
  const [project, setProject] = useState("");
  const [key, setKey] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [disconnecting, setDisconnecting] = useState<TrackerConnection | null>(null);
  const [histories, setHistories] = useState<Record<string, TrackerHistory>>({});
  const base = `/api/projects/${encodeURIComponent(p.id)}/trackers`;
  const changes = !p.team || p.team.role !== "viewer";
  const load = () =>
    call<TrackerConnections>("GET", base).then(
      (v) => setConns(v.connections),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [p.id]);
  // A connection's history is read again when what it counts changes.
  const stamp = (conns ?? []).map((c) => `${c.id}:${c.purge_after === null}:${c.failed_moves}:${c.waiting_writes}`).join(",");
  useEffect(() => {
    for (const c of conns ?? []) {
      if (c.purge_after !== null) continue;
      call<TrackerHistory>("GET", `${base}/${c.id}/history`).then(
        (h) => setHistories((all) => ({ ...all, [c.id]: h })),
        () => {},
      );
    }
  }, [p.id, stamp]);
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
    const body = { workspace_slug: workspace.trim(), project: project.trim(), api_key: key.trim() };
    // The key leaves the form as it's sent, whatever the answer.
    setKey("");
    void act(async () => {
      await call("POST", base, body);
      setProject("");
    });
  };
  const list = useBoards(p.id);
  // The open boards each connection's epics made; a connection's name is what the board's source carries.
  const following = (c: TrackerConnection) => (Array.isArray(list) ? list.filter((b) => b.closed_at == null && b.source?.connection === c.name).length : null);
  const connected = (conns ?? []).filter((c) => c.purge_after === null).length;
  const waiting = (conns ?? []).reduce((n, c) => n + (c.purge_after === null ? c.waiting_writes : 0), 0);
  const failedMoves = (conns ?? []).reduce((n, c) => n + (c.purge_after === null ? c.failed_moves : 0), 0);
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
          <p>
            Without a webhook, changes arrive within 5 minutes. A webhook makes them instant. Once the project is connected, copy its webhook URL from the table, then in Plane
            open Workspace settings → Webhooks → Add webhook. Paste the URL, choose individual events and tick <b>Work items</b> only, and create it. Plane then shows a secret:
            paste it into the connection's Webhook cell here. Plane sends every project's events to one webhook, and Arugula ignores the other projects'.
          </p>
          <p>
            <a href="https://docs.arugula.io/plane/" target="_blank" rel="noreferrer">
              More about Plane in Arugula
            </a>
          </p>
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
      {failedMoves > 0 && (
        <p class="control-error" data-plane-failed>
          {failedMoves} failed: Arugula couldn't move {failedMoves === 1 ? "an item" : "items"} to Done. Retry {failedMoves === 1 ? "it" : "them"} under recent changes.
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
                <th scope="col">Webhook</th>
                <th scope="col">Epics</th>
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
                      <div class="plane-policy" data-plane-policy={c.id}>
                        <select
                          aria-label={`What agents may write to ${c.identifier}`}
                          data-plane-writes={c.id}
                          value={presetOf(c.write_policy)}
                          disabled={busy || !changes}
                          onChange={(e) => {
                            const preset = PRESETS.find((x) => x.name === e.currentTarget.value);
                            if (preset) void act(() => call("PATCH", `${base}/${c.id}`, { write_policy: preset.policy }));
                          }}
                        >
                          {presetOf(c.write_policy) === CUSTOM && <option value={CUSTOM}>{CUSTOM}</option>}
                          {PRESETS.map((x) => (
                            <option value={x.name}>{x.name}</option>
                          ))}
                        </select>
                        <ul class="plane-kinds">
                          {KINDS.map(({ kind, label }) => (
                            <li>
                              <label>
                                <span>{label}</span>
                                <select
                                  aria-label={`${label}, ${c.identifier}`}
                                  data-plane-kind={kind}
                                  value={c.write_policy[kind]}
                                  disabled={busy || !changes}
                                  onChange={(e) => {
                                    const write_policy = { ...c.write_policy, [kind]: e.currentTarget.value };
                                    void act(() => call("PATCH", `${base}/${c.id}`, { write_policy }));
                                  }}
                                >
                                  <option value="allow">Allow</option>
                                  <option value="approve">Approve</option>
                                </select>
                              </label>
                            </li>
                          ))}
                        </ul>
                        <label class="plane-attribution">
                          <span>Footer</span>
                          <select
                            data-plane-attribution={c.id}
                            value={c.attribution}
                            disabled={busy || !changes}
                            onChange={(e) => {
                              const attribution = e.currentTarget.value;
                              void act(() => call("PATCH", `${base}/${c.id}`, { attribution }));
                            }}
                          >
                            {ATTRIBUTIONS.map((a) => (
                              <option value={a.value}>{a.label}</option>
                            ))}
                          </select>
                        </label>
                        <span class="dim" data-plane-attribution-help>
                          {attributionHelp(c.attribution)}
                        </span>
                      </div>
                    )}
                  </td>
                  <td data-col="Webhook">
                    {c.purge_after === null && (
                      <WebhookCell c={c} changes={changes} busy={busy} save={(webhook_secret) => act(() => call("PATCH", `${base}/${c.id}`, { webhook_secret }))} />
                    )}
                  </td>
                  <td data-col="Epics">
                    {c.purge_after === null && (
                      <div class="plane-epics" data-plane-epics={c.id}>
                        <label class="settings-switch">
                          <input
                            type="checkbox"
                            role="switch"
                            class="sw-switch"
                            data-plane-epics-switch={c.id}
                            checked={c.epics_as_boards}
                            disabled={busy || !changes}
                            onChange={(e) => {
                              const epics_as_boards = e.currentTarget.checked;
                              void act(() => call("PATCH", `${base}/${c.id}`, { epics_as_boards }));
                            }}
                          />
                          <span>Epics become boards</span>
                        </label>
                        <span class="dim">Each open epic in this Plane project gets a board that keeps itself up to date. Off, the boards stay but stop following.</span>
                        {list !== "off" && following(c) !== null && (
                          <span class="dim" data-plane-epics-count>
                            {epicsLine(following(c)!)}
                          </span>
                        )}
                      </div>
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
      {(conns ?? [])
        .filter((c) => c.purge_after === null)
        .map((c) => (
          <RecentChanges
            key={c.id}
            c={c}
            history={histories[c.id]}
            changes={changes}
            busy={busy}
            retry={(job) =>
              void act(async () => {
                const h = await call<TrackerHistory>("POST", `${base}/${c.id}/moves/${job}/retry`);
                setHistories((all) => ({ ...all, [c.id]: h }));
              })
            }
          />
        ))}
      {changes ? (
        <form class="plane-connect" aria-label="Connect a Plane project" data-plane-form onSubmit={submit}>
          <h3>Connect a Plane project</h3>
          <label class="plane-connect-field">
            <span class="plane-connect-label">Workspace</span>
            <input class="field" required placeholder="my-team" autocomplete="off" value={workspace} data-plane-workspace onInput={(e) => setWorkspace(e.currentTarget.value)} />
            <span class="dim">The slug in your Plane address: app.plane.so/<b>my-team</b>/…</span>
          </label>
          <label class="plane-connect-field">
            <span class="plane-connect-label">Project</span>
            <input class="field" required placeholder="ABC" autocomplete="off" value={project} data-plane-project onInput={(e) => setProject(e.currentTarget.value)} />
            <span class="dim">Its identifier (the ABC in ABC-12), or its id from the project's address.</span>
          </label>
          <label class="plane-connect-field">
            <span class="plane-connect-label">API key</span>
            <input class="field" required type="password" autocomplete="off" value={key} data-plane-key onInput={(e) => setKey(e.currentTarget.value)} />
            <span class="dim">From Plane's Profile settings, under API tokens. It's sealed here and never shown again.</span>
          </label>
          <span class="plane-connect-buttons">
            <button class="primary" disabled={busy}>
              Connect
            </button>
          </span>
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
