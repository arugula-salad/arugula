// Developer settings (#665, on #464's flags): this machine's Labs flags, one
// switch each. They're the daemon's (`flags.json` in its state directory),
// so the list is the shown host's, and the owner's alone: the daemon refuses
// anyone else. Offered in the menus once the machine has asked for it
// (`arugulad flags`); the desktop app's Daemon menu and `#developer` open it
// either way.

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import type { Client } from "../client";
import type { FlagInfo } from "../../shared/proto.gen";

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

const onOff = (on: boolean) => (on ? "on" : "off");

/** Whether control has a word on it for this machine (#347). */
const controlSays = (f: FlagInfo) => f.account !== undefined || f.team !== undefined;

/** Taking this machine's word back: one action, named by what it leaves
 * (UI audit #175: "Follow control" and "Reset" were the same). */
const followLabel = (f: FlagInfo) => (controlSays(f) ? "Use control's" : "Use the default");

/** Where a flag's state comes from (#347): this machine, the account or
 * the team on control, or its default. A daemon from before says nothing. */
function FlagSource({ f }: { f: FlagInfo }) {
  if (!f.source) return null;
  const control = f.account !== undefined ? `${onOff(f.account)} for your account` : f.team !== undefined ? `${onOff(f.team)} for your team` : null;
  // One pattern, as the Teams dialog's folds say it: "Set for … on control".
  const text =
    f.source === "machine"
      ? `Set on this machine${control ? `; control has it ${control}` : ""}`
      : f.source === "account"
        ? "Set for your account on control"
        : f.source === "team"
          ? "Set for your team on control"
          : null;
  return text ? (
    <span class="hint flag-source" data-flag-source={f.source}>
      {text}
    </span>
  ) : null;
}

// A Modal like control's dialogs (UI audit #175).
function Developer({ client, close }: { client: Client; close: () => void }) {
  const [flags, setFlags] = useState<FlagInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    try {
      const res = await client.request("GET", "/api/flags");
      // Not JSON: something other than the daemon answered (a page, a proxy).
      const v = await res.json<FlagInfo[] | { error?: string }>().catch(() => null);
      if (res.status === 404) throw new Error("This machine's daemon is too old for flags: update it");
      if (v === null) throw new Error(`This machine's daemon didn't answer with its flags (${res.status}): reopen this once it's connected`);
      if (!res.ok || !Array.isArray(v)) throw new Error((v as { error?: string }).error ?? `couldn't read the flags (${res.status})`);
      setFlags(v);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  const set = async (name: string, on: boolean | null) => {
    const res = await client.request("PUT", `/api/flags/${encodeURIComponent(name)}`, { on }).catch(() => null);
    if (!res?.ok) setError("couldn't change it");
    // The menus and the page follow the host's features: no reload.
    await Promise.all([load(), client.loadFeatures()]);
  };

  return (
    <Modal close={close} label="Developer settings" class="rules developer">
        <h2>Developer settings</h2>
        <p class="hint">
          Features that aren't ready for everyone, on this machine. Each takes effect at once; <code>arugulad flags</code> shows the same
          switches. What you set here wins over what control sets for your account or team.
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
                  <FlagSource f={f} />
                  {f.local_only ? (
                    <span class="hint flag-source" data-flag-local-only>
                      Only this machine sets it, never control
                    </span>
                  ) : null}
                </span>
              </label>
              {/* This machine's word taken back: control's, or the default. */}
              {f.source && f.local !== undefined ? (
                <button type="button" class="secondary small flag-follow" aria-label={`${followLabel(f)} for ${f.title || f.name}`} onClick={() => void set(f.name, null)}>
                  {followLabel(f)}
                </button>
              ) : null}
            </li>
          ))}
        </ul>
        <div class="prompt-buttons">
          <button class="secondary" type="button" onClick={close}>
            Done
          </button>
        </div>
    </Modal>
  );
}
