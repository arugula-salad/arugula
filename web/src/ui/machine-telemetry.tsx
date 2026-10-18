// The machine's usage and crash reports (M103, the telemetry track): its
// owner's yes or no, which the daemon keeps (`telemetry.get`/`telemetry.set`,
// M101), asked once on the web client and changed in the machine's settings.
//
// - **The question** is the card (telemetry-card.tsx), shown to the owner
//   alone, only on a page talking to the daemon directly, while the daemon
//   says nobody has answered. Control's page asks about the account
//   instead (M104), and asks nothing about the machine: one question is
//   enough there, and the machine's switch is in its settings.
// - **The switch** is *Usage and crash reports…* in the machine's settings
//   (the machine menu, or the account menu's *This machine*), and on the
//   phone's sheet, with the same preview.
// - **Views** this page opens (telemetry.ts) go to that daemon's
//   `telemetry.note` each minute and as the page goes away, while the
//   machine's answer is yes. Control's page sends its own to control (M104),
//   never here, so no view counts twice.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { desktopApp } from "../desktop";
import { OPS, opPath } from "../ops";
import type { TelemetryInfo } from "../proto.gen";
import { automated, counting, flush, startCounting, stopCounting, type PageCounts } from "../telemetry";
import { useSubscribe } from "./hooks";
import { TelemetryCard, TelemetryPreview } from "./telemetry-card";

const ABOUT =
  "Send usage counts and crash reports from this machine: versions, which features are used and how often, and where our code crashed. Never what your terminals show, file names, commands or prompts.";

/** The client shown, and whether this page reaches it directly (not as
 * control's page, nor a read-only link through control). */
let followed: Client | null = null;
let direct = false;
/** Its `telemetry.get`, once read (null: not yet, not the owner, or a
 * daemon from before M101). */
let info: TelemetryInfo | null = null;
let readFor: Client | null = null;
/** The card was put away unanswered on this load: asked again next load. */
let later = false;
/** The settings dialog's machine, while it's open. */
let dialog: Client | null = null;
let unsubscribe = () => {};
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

const owner = (c: Client) => c.connected && !!c.state && !c.state.roles;

/** Where the answer was given. */
const via = () => (desktopApp() ? "desktop" : "web");

/** This page's counts to `c`'s daemon (`telemetry.note`), a `keepalive`
 * request as the page goes away. A 403 (not the owner now) stops them. */
const noteTo =
  (c: Client) =>
  async (counts: PageCounts, leaving: boolean): Promise<boolean> =>
    (await c.request(OPS["telemetry.note"].method, opPath("telemetry.note"), counts, { keepalive: leaving })).status !== 403;

/** Follow the shown machine's client: read its answer once its owner is
 * connected, and count for it while that's yes. `isDirect`: this page
 * talks to the daemon itself. */
export function followMachine(client: Client, isDirect: boolean) {
  if (followed === client) return;
  unsubscribe();
  // The machine before: what it counted goes to it, then no more.
  if (direct && counting()) {
    flush(true);
    stopCounting();
  }
  followed = client;
  direct = isDirect;
  info = null;
  readFor = null;
  const check = () => {
    if (followed !== client || readFor === client || !owner(client)) return;
    readFor = client;
    void read(client).then((i) => {
      if (followed !== client) return;
      info = i;
      count();
      changed();
    });
  };
  unsubscribe = client.subscribe(check);
  check();
  changed();
}

async function read(c: Client): Promise<TelemetryInfo | null> {
  const res = await c.request(OPS["telemetry.get"].method, opPath("telemetry.get")).catch(() => null);
  return res?.ok ? await res.json<TelemetryInfo>().catch(() => null) : null;
}

/** The owner's answer for `c`'s machine; what its daemon says now. */
async function answer(c: Client, on: boolean): Promise<TelemetryInfo> {
  const res = await c.request(OPS["telemetry.set"].method, opPath("telemetry.set"), { on, via: via() });
  if (!res.ok) throw new Error(`couldn't change it (${res.status})`);
  const i = await res.json<TelemetryInfo>();
  if (c === followed) {
    info = i;
    count();
  }
  changed();
  return i;
}

/** Count this page's views for the followed machine while it says yes. */
function count() {
  if (!direct) return;
  if (info?.on) {
    if (!counting() && followed) startCounting(noteTo(followed));
  } else if (counting()) stopCounting();
}

export function openMachineTelemetry(client: Client) {
  dialog = client;
  changed();
}

/** The card, and the settings dialog when open. */
export function MachineTelemetryLayer() {
  // The answer can come before this effect runs: useSubscribe catches up.
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => listeners.delete(fn);
  });
  if (dialog)
    return (
      <MachineTelemetrySettings
        key={dialog.base}
        client={dialog}
        close={() => {
          dialog = null;
          changed();
        }}
      />
    );
  const c = followed;
  if (!c || !direct || !info || info.why !== "not_asked" || later || automated() || !owner(c)) return null;
  return (
    <TelemetryCard
      about={ABOUT}
      preview={info.preview}
      answer={(on) => void answer(c, on).catch(() => {})}
      later={() => {
        later = true;
        changed();
      }}
    />
  );
}

/** Why it's off whatever the owner says, when something keeps it off. */
const KEPT_OFF: Partial<Record<TelemetryInfo["why"], string>> = {
  do_not_track: "Off on this machine while DO_NOT_TRACK is set, whatever you choose here.",
  env: "Off on this machine while ARUGULA_TELEMETRY says off, whatever you choose here.",
  ci: "Off on this machine while CI is set, whatever you choose here.",
  test: "Off: this daemon is a test run.",
};

function MachineTelemetrySettings({ client, close }: { client: Client; close: () => void }) {
  const [t, setT] = useState<TelemetryInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void read(client).then((i) => (i ? setT(i) : setError("This machine's daemon didn't say: it may be too old for usage reports.")));
  }, []);
  const kept = t ? KEPT_OFF[t.why] : undefined;
  const set = (on: boolean) => {
    setError(null);
    answer(client, on).then(setT, (e: Error) => setError(e.message));
  };
  return (
    <div
      class="prompt-backdrop"
      role="dialog"
      aria-label="Usage and crash reports"
      onPointerDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class="prompt rules machine-telemetry" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Usage and crash reports</h2>
        <p class="hint">{ABOUT}</p>
        {error && <p class="error">{error}</p>}
        {kept && <p class="hint">{kept}</p>}
        {t && (
          <>
            <label class="machine-telemetry-switch">
              <input
                type="checkbox"
                role="switch"
                data-machine-telemetry-switch
                checked={t.on}
                disabled={!!kept}
                onChange={(e) => set(e.currentTarget.checked)}
              />{" "}
              Usage and crash reports from this machine
            </label>
            {t.preview !== undefined && <TelemetryPreview preview={t.preview} />}
          </>
        )}
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
