// Notifications on this device (#95, #96): one state machine for the
// account menu and the phone's sheet. Whether this device can be notified,
// turning it on and off, and what's in the way when it can't.

import { OPS, opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import {
  canTestPush,
  disablePush,
  enablePush,
  ios,
  onPushBackend,
  pushCheck,
  pushNote,
  pushPrefs,
  pushState,
  pushTest,
  setPushPrefs,
  standalone,
  type PushPrefs,
  type PushState,
} from "../push";
import { HOME_SCREEN, blockerOf, healthOf, standingSaid, testSaid, type PushStanding } from "../push-health";
import type { MenuItem } from "../../shared/ui/menu";
import { CopyText } from "../../shared/ui/copy";
import { openGettingStarted } from "./welcome";

let push: PushState = "unsupported";
/** What control pushes the account about; null off control, or not yet asked. */
let prefs: PushPrefs | null = null;
/** #468: how the account's subscriptions stand (control's), once asked. */
let standing: PushStanding | null = null;
/** What the last test notification did, in words; null before one. */
let tested: string | null = null;
let testing = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
/** Ask again when another backend takes over (the desktop app's). */
const reload = () =>
  void pushState().then((s) => {
    push = s;
    changed();
    if (s === "on") void loadPrefs();
    void loadStanding();
  });
reload();
onPushBackend(reload);

async function loadPrefs() {
  prefs = await pushPrefs().catch(() => null);
  changed();
}

/** Control's word on the account's subscriptions (#468). Asking may hand
 * this device's over again (main.tsx), so the state is read after. */
async function loadStanding() {
  if (!canTestPush()) return;
  standing = await pushCheck().catch(() => standing);
  changed();
}

/** The browser's notification permission, when it has one. */
const permission = () => ("Notification" in window ? Notification.permission : null);

/** What's wrong with this device's notifications (#468), or null. */
export const notifyHealth = () => healthOf(push, standing, permission());

/** *Send a test notification* (#468): to every subscription the account
 * has; what became of it shows beside the button and as a toast. */
export async function sendTestPush(client: Client) {
  if (testing) return;
  testing = true;
  changed();
  try {
    const r = await pushTest();
    if (r) {
      standing = r;
      tested = testSaid(r);
      client.toast(tested);
    }
  } catch (e) {
    tested = String(e instanceof Error ? e.message : e);
    client.toast(tested);
  } finally {
    testing = false;
    changed();
  }
}

export const pushNow = () => push;
/** What control pushes the account about, once asked (null off control). */
export const pushKinds = () => prefs;
export function subscribePush(fn: () => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** What puts this daemon on the tailnet over https (the phone's way in). */
export function serveCommand(): string {
  const port = location.protocol === "http:" && location.port ? location.port : "7681";
  return `tailscale serve --bg --https=443 http://127.0.0.1:${port}`;
}

/** Why this device can't be notified, when it can't. */
export function notifyBlocker(s: PushState = push): string | null {
  return blockerOf(s);
}

/** Turn this device's notifications on or off, as the menu's switch does. */
export async function toggleNotify(client: Client) {
  try {
    if (push === "on") push = await disablePush();
    else {
      push = await enablePush();
      // Prove it: the daemon sends this device one now.
      if (push === "on") void fetch(opPath("push.test"), { method: OPS["push.test"].method }).catch(() => {});
      if (push === "on") void loadPrefs();
    }
  } catch (e) {
    client.toast(String(e));
  }
  void loadStanding();
  changed();
  client.emit();
}

/** Notify this device, unless it is already (Getting started's Things to
 * try). */
export async function turnOnPush(client: Client) {
  if (push !== "on") await toggleNotify(client);
}

/** Turn this device's notifications on (a hint's action, shell/hints). */
export function notifyThisDevice(client: Client) {
  if (push === "off") void toggleNotify(client);
}

/** The kinds of push an account chooses on control, as the menu says them. */
const KINDS: { key: keyof PushPrefs; label: string; hint: string }[] = [
  { key: "chat", label: "Chat", hint: "Messages in team channels, DMs and pane threads" },
  { key: "machines", label: "Machines", hint: "A pane or an agent on your machines that needs you" },
  { key: "projects", label: "Projects", hint: "Your inbox, deploys, runs and workspaces" },
];

async function setKind(client: Client, key: keyof PushPrefs, on: boolean) {
  try {
    prefs = (await setPushPrefs({ [key]: on })) ?? prefs;
  } catch (e) {
    client.toast(String(e));
  }
  listeners.forEach((fn) => fn());
  client.emit();
}

/** #468: *Send a test notification*, on control: to every subscription the
 * account has, with how many there are. */
function testItem(client: Client): MenuItem[] {
  if (!canTestPush()) return [];
  return [
    {
      label: testing ? "Sending a test notification…" : "Send a test notification",
      hint: standing ? `${standingSaid(standing)} The test goes to every one.` : "To every device of yours with notifications on",
      disabled: testing,
      run: () => void sendTestPush(client),
    },
  ];
}

/** This device's switch, then (on control, while it's on) what to push
 * about, for every device that's on. Notices about the account (a device
 * waiting, a new agent token) always go. */
function switchItems(client: Client): MenuItem[] {
  // The desktop app's: only while it runs (crates/desktop/src/notices.rs).
  const note = pushNote();
  const device: MenuItem = { label: note ? `Notify this device (${note})` : "Notify this device", checked: push === "on", run: () => void toggleNotify(client) };
  const p = push === "on" ? prefs : null;
  if (!p) return [device];
  return [
    device,
    ...KINDS.map(({ key, label, hint }): MenuItem => ({
      label: `Push: ${label}`,
      hint: `${hint}. On every device you turned on.`,
      checked: p[key],
      run: () => void setKind(client, key, !p[key]),
    })),
  ];
}

export function notificationItems(client: Client): MenuItem[] {
  const blocked = notifyBlocker();
  // Plain http: the fix is Tailscale, a button in Getting started's phone step.
  if (push === "insecure") return [{ label: `${blocked}…`, run: () => openGettingStarted("phone") }];
  if (blocked) return [{ label: blocked, disabled: true, run: () => {} }, ...testItem(client)];
  const [device, ...kinds] = switchItems(client);
  // #468: what's wrong with this device's, said plainly (on control).
  const health = canTestPush() ? notifyHealth() : null;
  const said: MenuItem[] = health ? [{ label: health, disabled: true, run: () => {} }] : [];
  return [device, ...said, ...kinds, ...testItem(client)];
}

/** M29: someone other than the owner chooses which agents they're told
 * about: this session's, or everything they may answer here ("this team's
 * agents" on a team daemon). The owner always is. */
export function agentNotifyItems(client: Client, session: number): MenuItem[] {
  if (!client.state?.roles || client.role(session) === "viewer") return [];
  const pref = client.notifyPref;
  const set = (body: { session?: number; on: boolean }) => void client.setNotify(body);
  return [
    {
      label: "Notify me about its agents",
      checked: !!pref && (pref.all || pref.sessions.includes(session)),
      run: () => set({ session, on: !(pref?.sessions.includes(session) ?? false) }),
    },
    {
      label: "Notify me about every agent here",
      checked: !!pref?.all,
      run: () => set({ on: !pref?.all }),
    },
  ];
}

/** The same items in the phone's sheet (#95): toggles, or why it can't. */
export function NotifySection({ client, session }: { client: Client; session: number | null }) {
  const [, setTick] = useState(0);
  useEffect(() => subscribePush(() => setTick((t) => t + 1)), []);
  const blocked = notifyBlocker();
  // The toggles; the health line and the test are drawn as themselves below.
  const items = [
    ...(blocked ? [] : switchItems(client)),
    ...(session !== null ? agentNotifyItems(client, session) : []),
  ];
  const health = blocked || !canTestPush() ? null : notifyHealth();
  return (
    <section class="sheet-notify">
      <h2>Notifications</h2>
      {blocked && (
        <p class="sheet-note" data-notify-blocked={push}>
          {blocked}
          {push === "insecure" ? ". On this machine:" : "."}
        </p>
      )}
      {push === "insecure" && <CopyText text={serveCommand()} data-serve-command />}
      {health && (
        <p class="sheet-note" data-notify-health>
          {health}
        </p>
      )}
      {items.map(
        (item) =>
          typeof item === "object" &&
          "run" in item && (
            <button key={item.label} class="sheet-item sheet-toggle" aria-pressed={!!item.checked} onClick={item.run}>
              <span class="sheet-check">{item.checked ? "●" : "○"}</span> {item.label}
            </button>
          ),
      )}
      {canTestPush() && (
        <>
          <button class="sheet-item" data-push-test disabled={testing} onClick={() => void sendTestPush(client)}>
            {testing ? "Sending a test notification…" : "Send a test notification"}
          </button>
          <p class="sheet-note" data-push-standing>
            {tested ?? (standing ? standingSaid(standing) : "")}
          </p>
        </>
      )}
    </section>
  );
}

// ---------------------------------------------------------------- install hint

/** Android's install prompt, when the browser offers one. */
let installPrompt: (Event & { prompt(): Promise<void> }) | null = null;
window.addEventListener("beforeinstallprompt", (e) => {
  e.preventDefault();
  installPrompt = e as typeof installPrompt;
  listeners.forEach((fn) => fn());
});

const HINT_KEY = "arugula.install-hint";

function hintDismissed(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "dismissed";
  } catch {
    return false;
  }
}

/** A phone that isn't installed (#96): once, until dismissed, say how. On
 * iOS that's also the only way to get notifications. */
export function InstallHint() {
  const [gone, setGone] = useState(hintDismissed);
  const [, setTick] = useState(0);
  useEffect(() => subscribePush(() => setTick((t) => t + 1)), []);
  if (gone || standalone() || !isSecureContext || !(ios() || installPrompt)) return null;
  const dismiss = () => {
    try {
      localStorage.setItem(HINT_KEY, "dismissed");
    } catch {
      // Shown again next time; harmless.
    }
    setGone(true);
  };
  return (
    <div class="install-hint" role="status" data-install-hint>
      {installPrompt ? (
        <>
          <span>Install it for a full screen and notifications.</span>
          <button
            class="primary"
            onClick={() => {
              void installPrompt?.prompt();
              installPrompt = null;
              dismiss();
            }}
          >
            Install
          </button>
        </>
      ) : (
        <span>{HOME_SCREEN} (Share › Add to Home Screen), and open it from there.</span>
      )}
      <button class="install-hint-close" aria-label="Dismiss" onClick={dismiss}>
        ×
      </button>
    </div>
  );
}
