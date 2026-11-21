// Your devices (Arugula #1327), under This device in Settings › You ›
// Notifications: every phone, browser and desktop app that turned push
// on, last reached first, through the app's host
// (`/api/push/subscriptions`). Each has a switch and Remove. Off means
// nothing is sent to it, and it stays off when that device subscribes
// again; Remove deletes it until that device turns push on again. Turning
// one on is still that device's own (./you-push.tsx). While the hub
// answers 404 here, the list isn't drawn.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Time } from "../../shared/ui/time";

export type PushDevice = {
  id: string;
  label: string;
  kind: "browser" | "desktop" | "iphone";
  created: number | null;
  used: number | null;
  on: boolean;
};

const KINDS: Record<PushDevice["kind"], string> = { browser: "Browser", desktop: "Desktop app", iphone: "iPhone app" };

/** `/api/push/subscriptions…` through the app's host: its answer, or null for a 404. */
async function call<T>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown): Promise<T | null> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

const PATH = "/api/push/subscriptions";

export function YourDevices() {
  const [devices, setDevices] = useState<PushDevice[] | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
  // Remove asks first (UI audit #158).
  const [removing, setRemoving] = useState<PushDevice | null>(null);
  const load = () =>
    call<{ subscriptions: PushDevice[] }>("GET", PATH).then(
      (v) => setDevices(v ? v.subscriptions : null),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  // One that's gone (404: removed elsewhere, or not yours): the list again.
  const act = async (d: PushDevice, f: () => Promise<unknown>) => {
    setErr("");
    setBusy(d.id);
    try {
      if ((await f()) === null) await load();
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy("");
  };
  const flip = (d: PushDevice) =>
    act(d, async () => {
      const row = await call<PushDevice>("POST", `${PATH}/${encodeURIComponent(d.id)}`, { on: !d.on });
      if (row) setDevices((was) => was?.map((x) => (x.id === row.id ? row : x)) ?? null);
      return row;
    });
  const remove = (d: PushDevice) =>
    act(d, async () => {
      const done = await call<object>("DELETE", `${PATH}/${encodeURIComponent(d.id)}`);
      if (done) setDevices((was) => was?.filter((x) => x.id !== d.id) ?? null);
      return done;
    });
  if (devices === null) return err ? <p class="control-error">{err}</p> : null;
  return (
    <div data-push-devices>
      <h3 class="control-group">Your devices</h3>
      {devices.length === 0 ? <p class="dim">None has turned notifications on yet.</p> : null}
      <ul class="control-devices control-rows">
        {devices.map((d) => (
          <li key={d.id} data-push-device={d.id}>
            <label class="settings-switch">
              <input
                type="checkbox"
                role="switch"
                class="sw-switch"
                checked={d.on}
                disabled={busy === d.id}
                data-push-device-on={d.id}
                onChange={() => void flip(d)}
              />
              <span data-push-device-name>{d.label}</span>
            </label>
            <span class="dim">
              {KINDS[d.kind] ?? d.kind}
              {d.used ? (
                <>
                  , last used <Time at={d.used} on />
                </>
              ) : d.created ? (
                <>
                  , added <Time at={d.created} on />
                </>
              ) : null}
            </span>
            <button class="secondary danger small" data-push-device-remove={d.id} disabled={busy === d.id} onClick={() => setRemoving(d)}>
              Remove
            </button>
          </li>
        ))}
      </ul>
      <p class="dim">
        Off: nothing is sent to that device, and it stays off if it turns notifications on again. Remove: it's gone until that device turns
        notifications on again.
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      {removing && (
        <ConfirmRemove
          title={`Remove ${removing.label}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            const d = removing;
            setRemoving(null);
            void remove(d);
          }}
        >
          <p>Nothing is sent to it until that device turns notifications on again, which adds it back.</p>
        </ConfirmRemove>
      )}
    </div>
  );
}
