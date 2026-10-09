// Developer settings (#665, on #464's flags): this machine's Labs flags, one
// switch each. They're the daemon's (`flags.json` in its state directory),
// so the list is the shown host's, and the owner's alone: the daemon refuses
// anyone else. Offered in the menus once the machine has asked for it
// (`arugulad flags`); the desktop app's Daemon menu and `#developer` open it
// either way.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { FlagInfo } from "../proto.gen";

let shown: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openDeveloper(client: Client) {
  shown = client;
  changed();
}

export function DeveloperLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!shown) return null;
  return (
    <Developer
      client={shown}
      close={() => {
        shown = null;
        changed();
      }}
    />
  );
}

function Developer({ client, close }: { client: Client; close: () => void }) {
  const [flags, setFlags] = useState<FlagInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    try {
      const res = await client.request("GET", "/api/flags");
      const v = await res.json<FlagInfo[] | { error?: string }>();
      if (res.status === 404) throw new Error("This machine's daemon is too old for flags: update it");
      if (!res.ok || !Array.isArray(v)) throw new Error((v as { error?: string }).error ?? `HTTP ${res.status}`);
      setFlags(v);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  const set = async (name: string, on: boolean) => {
    const res = await client.request("PUT", `/api/flags/${encodeURIComponent(name)}`, { on }).catch(() => null);
    if (!res?.ok) setError("couldn't change it");
    // The menus and the page follow the host's features: no reload.
    await Promise.all([load(), client.loadFeatures()]);
  };

  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Developer settings" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt rules developer" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Developer settings</h2>
        <p class="hint">
          Features that aren't ready for everyone, on this machine. Each takes effect at once; <code>arugulad flags</code> shows the same
          switches.
        </p>
        {error && <p class="error">{error}</p>}
        {flags?.some((f) => f.built === false) && <p class="hint">This daemon was built without Labs: the switches turn nothing on.</p>}
        <ul class="rules-list">
          {(flags ?? []).map((f) => (
            <li key={f.name}>
              <label data-flag={f.name}>
                <input type="checkbox" checked={f.on} onChange={(e) => void set(f.name, e.currentTarget.checked)} />
                <span>
                  <b>{f.title || f.name}</b>
                  <span class="hint">{f.about}</span>
                </span>
              </label>
            </li>
          ))}
        </ul>
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
