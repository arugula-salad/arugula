// Labs and the other named flags (#464, #347): what this machine has turned
// on that a stranger doesn't get. They're the daemon's (`flags.json` in its
// state), so the list is the shown host's, and it's the owner's: the session
// menu offers it to nobody else, and the daemon refuses anyone else.
// A page reads `HostFeatures` and loads the Labs chunk when it starts, so a
// switch reloads the page to show what changed.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { FlagInfo } from "../proto.gen";

let shown: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openFlags(client: Client) {
  shown = client;
  changed();
}

export function FlagsLayer() {
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
    <Flags
      client={shown}
      close={() => {
        shown = null;
        changed();
      }}
    />
  );
}

function Flags({ client, close }: { client: Client; close: () => void }) {
  const [flags, setFlags] = useState<FlagInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const res = await client.request("GET", "/api/flags");
        const v = await res.json<FlagInfo[] | { error?: string }>();
        if (!res.ok || !Array.isArray(v)) throw new Error((v as { error?: string }).error ?? `HTTP ${res.status}`);
        setFlags(v);
      } catch (e) {
        setError((e as Error).message);
      }
    })();
  }, []);

  const set = async (name: string, on: boolean) => {
    const res = await client.request("PUT", `/api/flags/${encodeURIComponent(name)}`, { on }).catch(() => null);
    if (!res?.ok) return setError("couldn't change it");
    location.reload();
  };

  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Labs" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt rules" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Labs</h2>
        <p class="hint">
          Switched on for this machine. The page reloads to show the change; nothing restarts, and a flag follows the file as soon as it
          changes. <code>arugulad flags</code> does the same from a terminal.
        </p>
        {error && <p class="error">{error}</p>}
        <ul class="rules-list">
          {(flags ?? []).map((f) => (
            <li key={f.name} data-flag={f.name}>
              <span class="rule-text">
                <b>{f.name}</b>: {f.about}
                {f.built === false && <i> This build of Arugula has no {f.name} code, so turning it on changes nothing here.</i>}
              </span>
              <button aria-pressed={f.on} onClick={() => void set(f.name, !f.on)}>
                {f.on ? "On: turn off" : "Off: turn on"}
              </button>
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
