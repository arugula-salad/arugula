// A project's board settings (Arugula #404), a section of its settings after
// Plane: how long a Doing row's agent blocks may sit idle before the row is
// marked stalled. Owners and editors change it; a team's viewers read it.

import { useEffect, useState } from "preact/hooks";
import type { BoardSettings as Settings } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { Section } from "./section";

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

export function BoardSettings({ p }: { p: PlatformProject }) {
  const [minutes, setMinutes] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(p.id)}/board-settings`;
  const changes = !p.team || p.team.role !== "viewer";
  useEffect(() => {
    let live = true;
    call<Settings>("GET", base).then(
      (v) => live && (setMinutes(v.stall_minutes), setDraft(String(v.stall_minutes))),
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, [p.id]);
  const save = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setErr("");
    try {
      const v = await call<Settings>("PUT", base, { stall_minutes: Number(draft) });
      setMinutes(v.stall_minutes);
      setDraft(String(v.stall_minutes));
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  if (minutes === null && !err) return null;
  return (
    <Section
      id="boards"
      title="Boards"
      lede="When a row counts as stalled."
      more={<p>A row in Doing whose agents have all sat idle for longer than this, with no pull request on it, is marked stalled on its board, and waits in the Needs you of whoever owns the agent.</p>}
      hooks={{ "data-board-settings": p.id }}
    >
      <form class="settings-row" onSubmit={save}>
        <label>
          Stalled after{" "}
          <input type="number" min={1} max={10080} value={draft} disabled={!changes || busy} data-stall-minutes onInput={(e) => setDraft((e.target as HTMLInputElement).value)} /> minutes idle
        </label>
        {changes && (
          <button type="submit" class="secondary small" data-stall-save disabled={busy || draft === String(minutes)}>
            Save
          </button>
        )}
      </form>
      {err && <p class="control-error" data-board-settings-error>{err}</p>}
    </Section>
  );
}
