// Settings › You › Phones and browsers (Arugula #1310): what every device
// you turned on is pushed about (Chat, Machines, Projects, as the menu's
// switches say them: shared/push-kinds.ts), set in place through the
// app's host. This device's switch and Send a test stay on your machines
// page: a push subscription belongs to its service worker's origin, and
// that's control's (aru-081). A control that doesn't answer
// `/api/push/prefs` on this host yet (a bare 404) gets both as ↗ rows, as
// before.

import { useEffect, useState } from "preact/hooks";
import { PUSH_KINDS, type PushPrefs } from "../../shared/push-kinds";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { Section } from "../../shared/ui/settings-section";
import { controlOrigin } from "../session";
import { ControlRow } from "../settings/owner-machines";

/** `/api/push/prefs` through the app's host: the prefs, or null for a
 * bare 404 (the hub doesn't serve it here yet). */
async function prefsCall(body?: Partial<PushPrefs>): Promise<PushPrefs | null> {
  const r = await platformFetch("/api/push/prefs", {
    method: body ? "POST" : "GET",
    credentials: "same-origin",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const v = (await r.json().catch(() => ({}))) as PushPrefs & { error?: string; reason?: string };
  if (r.status === 404 && !v.error) return null;
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** What's pushed, in a few words: `Chat, Machines`. */
function pushedSaid(p: PushPrefs): string {
  const on = PUSH_KINDS.filter((k) => p[k.key]).map((k) => k.label);
  return on.length === PUSH_KINDS.length ? "Everything" : on.length ? on.join(", ") : "Nothing";
}

export function PhonesAndBrowsers() {
  const [prefs, setPrefs] = useState<PushPrefs | null>(null);
  const [elsewhere, setElsewhere] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    prefsCall().then(
      (p) => live && (p ? setPrefs(p) : setElsewhere(true)),
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, []);
  const push = `${controlOrigin()}/#settings=push`;
  const lede = "Which of your devices are notified, and what about.";
  const thisDevice = <ControlRow href={push} label="Notify this device" why="Turn this browser or phone on or off, and send a test notification" hook="push" />;
  if (elsewhere)
    return (
      <Section
        id="push"
        title="Phones and browsers"
        summary="On your machines page"
        lede={lede}
        more={<p>Each device turns itself on with its own keys, so these open your machines page.</p>}
      >
        <ul class="platform-list settings-rows">
          <ControlRow href={push} label="Notify this device" why="Turn this browser or phone on or off" hook="push" />
          <ControlRow href={push} label="What every device is pushed about" why="Chat, your machines, your projects; and a test notification" hook="push" />
        </ul>
      </Section>
    );
  const set = async (key: keyof PushPrefs, on: boolean) => {
    setErr("");
    setBusy(true);
    try {
      const p = await prefsCall({ [key]: on });
      if (p) setPrefs(p);
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy(false);
  };
  return (
    <Section
      id="push"
      title="Phones and browsers"
      summary={prefs ? `Pushed: ${pushedSaid(prefs)}` : err ? "Couldn't load" : undefined}
      lede={lede}
      more={<p>Each device turns itself on with its own keys, on your machines page. Notices about your account, like a device waiting, always go.</p>}
      hooks={{ "data-settings-push": "" }}
    >
      <ul class="platform-list settings-rows">{thisDevice}</ul>
      {prefs ? (
        <>
          <h3 class="control-group">What every device is pushed about</h3>
          <ul class="platform-list settings-rows" data-push-kinds>
            {PUSH_KINDS.map(({ key, label, hint }) => (
              <li key={key}>
                <label class="settings-switch">
                  <input
                    type="checkbox"
                    role="switch"
                    class="sw-switch"
                    checked={prefs[key]}
                    disabled={busy}
                    data-push-kind={key}
                    onChange={(e) => void set(key, e.currentTarget.checked)}
                  />
                  <span>{label}</span>
                </label>
                <span class="dim">{hint}</span>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </Section>
  );
}
