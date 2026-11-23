// The phone offer's Sign in (hints/phone, the onboarding prototype's tab
// 4b): the one-click cloud join Getting started does (#2626), in a tab of
// its own where they click Add, and this page carries on by itself: once
// the daemon says joined, a QR code for the app, to open it on the phone.
// Joined to their own account, the phone can answer and type (#2826); a
// team's only watches.

import { opPath } from "../ops";
import { Modal } from "../../shared/ui/modal";
import { useSubscribe } from "../../shared/ui/hooks";
import { Qr } from "./welcome";
import { refreshControlState } from "./control-state";

/** Arugula cloud, and the app a phone opens for it. */
const CONTROL = "https://control.arugula.io";
const APP = "https://app.arugula.io";
const POLL_MS = 2000;

type Step = { at: "signing"; approve?: string; error?: string } | { at: "phone"; url: string; team: boolean };

let step: Step | null = null;
let polling = 0;
const listeners = new Set<() => void>();
const set = (s: Step | null) => {
  step = s;
  listeners.forEach((fn) => fn());
};
const subscribe = (fn: () => void) => (listeners.add(fn), () => void listeners.delete(fn));

type Control = { url?: string; joined?: string; team?: string; pending?: { approve: string }; error?: string };

async function control(): Promise<Control | null> {
  const r = await fetch(`${opPath("setup.get")}?part=control`).catch(() => null);
  return r?.ok ? (((await r.json().catch(() => null)) as { control?: Control } | null)?.control ?? null) : null;
}

function joined(c: Control) {
  clearInterval(polling);
  refreshControlState();
  const url = !c.url || c.url === CONTROL ? APP : new URL(c.joined ?? c.url).origin;
  set({ at: "phone", url, team: !!c.team });
}

/** Sign in: join this machine, in a tab where they click Add. The tab is
 * opened now, in the click, so no popup blocker stops it. */
export async function signInForPhone() {
  clearInterval(polling);
  const tab = window.open("", "_blank");
  if (tab) tab.opener = null;
  set({ at: "signing" });
  const c = await control();
  if (c?.joined) {
    tab?.close();
    return joined(c);
  }
  const r: Control = await fetch(opPath("setup.control"), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ url: c?.url || CONTROL }),
  })
    .then((r) => r.json())
    .catch((e) => ({ error: String(e) }));
  const approve = r.pending?.approve ?? c?.pending?.approve;
  if (!approve) {
    tab?.close();
    return set({ at: "signing", error: r.error ?? "Arugula cloud didn't answer. Try again in a moment." });
  }
  if (tab) tab.location.href = approve;
  set({ at: "signing", approve });
  polling = window.setInterval(async () => {
    const now = await control();
    if (now?.joined) joined(now);
  }, POLL_MS);
}

function close() {
  clearInterval(polling);
  set(null);
}

/** Draws the sign-in and the phone's QR code while one is under way. Once,
 * beside the hint toast. */
export function PhoneOfferLayer() {
  useSubscribe(subscribe);
  const s = step;
  if (!s) return null;
  if (s.at === "signing")
    return (
      <Modal title="Signing in" close={close} root={{ "data-phone-signin": "" }} class="phone-offer">
        {s.error ? (
          <p data-phone-error>{s.error}</p>
        ) : (
          <p class="start-dim">
            <span class="start-pulse" /> Finish in your browser. This page carries on by itself.
          </p>
        )}
        {s.approve && (
          <p class="start-dim">
            No tab opened?{" "}
            <a href={s.approve} target="_blank" rel="noreferrer" data-phone-approve>
              Open Arugula cloud
            </a>
          </p>
        )}
      </Modal>
    );
  return (
    <Modal
      title="Open Arugula on your phone"
      close={close}
      root={{ "data-phone-qr": "" }}
      class="phone-offer"
      buttons={
        <button class="primary" onClick={close} data-phone-done>
          Done
        </button>
      }
    >
      <p class="start-dim">
        {s.team
          ? "Scan this, then sign in with the same account. This pane is there to watch, and your phone tells you when the next one waits."
          : "Scan this, then sign in with the same account. This pane and its prompt are there: you can answer and type from your phone, and it tells you when the next one waits."}
      </p>
      <Qr text={s.url} />
      <p class="phone-offer-url">{new URL(s.url).host}</p>
    </Modal>
  );
}
