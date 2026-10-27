// Control's overlay over the app (M17): one thing at a time, in order of
// what needs an answer first (recovery codes to keep, a link's join, device
// or invite; a device or machine asking; a session offered; what control
// kept to say), else the dialog the account menu opened, else the one-time
// usage counts question. Also the account's and the host menu's items.
//
// The platform's settings (plan and usage) are on the platform page
// (M153); the account menu links out to them, and offers none without one
// (M155). The usage reports question is the account's, drawn
// by ../../shared/ui/account-telemetry from what's passed it (M149).

import { useEffect, useState } from "preact/hooks";
import { openPlatform, platformOrigin } from "../platform-links";
import { TelemetryAsk } from "../../shared/ui/account-telemetry";
import type { MenuItem } from "../../shared/ui/menu";
import { Modal } from "../../shared/ui/modal";
import { ADD_PASSKEY_HASH, DEVICE_HASH, inviteInHash, JOIN_HASH, type ControlSession } from "../control";
import { SharedDocs } from "../ui/shared-docs";
import { AccountPanel, GITHUB_LINK_HASH } from "./account";
import { DeviceLinkPrompt, DevicePrompt, JoinPrompt } from "./approve";
import { DEPLOY_HASH, DeployPrompt } from "./deploy";
import { openPanel, useControl, usedLink, type Panel } from "./common";
import { AddDevice, Devices } from "./devices";
import { AddMachine } from "./machines";
import { AddPasskeyAgain, AdmitPrompt, AppLoginDone, AppLoginPrompt, ConfirmPasskey, InvitePrompt, Joined, Notice, PresignedPrompt, ShareOfferPrompt } from "./prompts";
import { RecoveryCodes } from "./recovery";
import { Teams } from "./teams";

/** Over the app: approval prompts, a daemon's join, the device list. */
export function ControlOverlay({ s }: { s: ControlSession }) {
  useControl(s);
  const [hash, setHash] = useState(location.hash);
  const [panel, setPanel] = useState<null | Panel>(null);
  useEffect(() => {
    const on = () => setHash(location.hash);
    const open = (e: Event) => setPanel((e as CustomEvent<Panel>).detail);
    addEventListener("hashchange", on);
    addEventListener("arugula:control-panel", open);
    // Says the panel event has a listener, so a test can wait for it
    // rather than send one into nothing.
    document.documentElement.dataset.controlPanels = "";
    // Mounted afresh (the page switches machine when one is approved, and
    // that happens before the prompt clears the hash): catch up with a
    // hashchange that fired before this listener was there.
    on();
    return () => {
      removeEventListener("hashchange", on);
      removeEventListener("arugula:control-panel", open);
      delete document.documentElement.dataset.controlPanels;
    };
  }, []);
  if (s.phase !== "ready") return null;
  if (s.recoveryCodes) return <RecoveryCodes s={s} />;
  if (s.passkeyToConfirm) return <ConfirmPasskey s={s} />;
  if (hash === ADD_PASSKEY_HASH) return <AddPasskeyAgain s={s} />;
  const join = JOIN_HASH.exec(hash);
  if (join) return <JoinPrompt s={s} code={join[1]} device={join[2]} />;
  // #761: a waiting device's code, scanned with the camera or opened.
  const device = DEVICE_HASH.exec(hash)?.[1];
  if (device) return <DeviceLinkPrompt key={device} s={s} whole={device} />;
  // The app's device asking to be approved comes first: it's the next step
  // after allowing its sign-in.
  const appLogin = /^#app=([0-9a-f]{16,128})$/.exec(hash)?.[1];
  if (appLogin && !s.pending[0]) return <AppLoginPrompt s={s} id={appLogin} />;
  if (hash === "#app-done" && !s.pending[0]) return <AppLoginDone />;
  // M218: a deploy's approval, from the platform page's Approve or Deny.
  const deploy = DEPLOY_HASH.exec(hash)?.[1];
  if (deploy) return <DeployPrompt key={deploy} s={s} id={deploy} />;
  const invite = inviteInHash(hash) ?? inviteInHash(usedLink);
  if (invite)
    return invite.presigned ? (
      <PresignedPrompt s={s} team={invite.team} seed={invite.code} />
    ) : (
      <InvitePrompt s={s} team={invite.team} code={invite.code} old={invite.old} />
    );
  // Someone used an invite to a team I own: add them (sign the roster)?
  const req = s.teams.flatMap((t) => (t.role === "owner" ? t.requests.map((r) => ({ t, r })) : []))[0];
  if (req) return <AdmitPrompt s={s} team={req.t} req={req.r} />;
  const asking = s.pending[0];
  // #326: a device that came to approve a machine's join: both together.
  const asked = asking && s.pendingJoins.get(asking.device);
  if (asking && asked) return <JoinPrompt key={asked} s={s} code={asked} from={asking} />;
  if (asking) return <DevicePrompt key={asking.device} s={s} c={asking} />;
  const offer = s.offers[0];
  if (offer) return <ShareOfferPrompt s={s} o={offer} />;
  if (s.joined) return <Joined s={s} />;
  const notice = s.notices[0];
  if (notice) return <Notice s={s} n={notice} />;
  // Back from linking GitHub (#208): Sign-in and account says how it went.
  const linked = GITHUB_LINK_HASH.exec(hash)?.[1];
  if (linked) {
    const done = () => {
      history.replaceState(history.state, "", location.pathname + location.search);
      setHash("");
    };
    return (
      <Modal close={done}>
        <AccountPanel s={s} close={done} linked={linked} />
      </Modal>
    );
  }
  if (panel === "devices") return <Devices s={s} close={() => setPanel(null)} />;
  if (panel === "teams") return <Teams s={s} close={() => setPanel(null)} />;
  if (panel === "shared-docs")
    return (
      <Modal close={() => setPanel(null)}>
        <SharedDocs s={s} close={() => setPanel(null)} />
      </Modal>
    );
  if (panel === "account")
    return (
      <Modal close={() => setPanel(null)}>
        <AccountPanel s={s} close={() => setPanel(null)} />
      </Modal>
    );
  if (panel === "add-device") return <AddDevice s={s} close={() => setPanel(null)} />;
  if (panel === "add")
    return (
      <Modal close={() => setPanel(null)}>
        <h2>Add a machine</h2>
        <AddMachine s={s} />
        {/* UI audit #150: every dialog ends the same way. */}
        <div class="prompt-buttons">
          <button class="secondary" onClick={() => setPanel(null)}>Done</button>
        </div>
      </Modal>
    );
  // M104: asked once, when nothing else is.
  if (s.askTelemetry())
    return (
      <TelemetryAsk answer={(on) => void s.setTelemetry(on).catch(() => {})} later={() => s.telemetryNotNow()} />
    );
  return null;
}

/** For the host menu. */
export function controlMenuItems(s: ControlSession): MenuItem[] {
  return [
    "separator",
    ...(s.stale ? [{ header: "Control unreachable" } as MenuItem] : []),
    { label: "Add a machine…", run: openPanel("add") },
  ];
}

/** The account's own items, in the account menu on every page (the bar's,
 * and the no-machines screen's, #97). Sign out is the menu's own, last. */
export function accountItems(s: ControlSession): MenuItem[] {
  return [
    { label: "Teams…", run: openPanel("teams") },
    { label: "Devices and machines…", run: openPanel("devices") },
    // #1066; everyone's since Docs left Labs (native #204).
    { label: "Shared Docs…", run: openPanel("shared-docs") },
    { label: "Add a phone or browser…", run: openPanel("add-device") },
    { label: "Sign-in and account…", run: openPanel("account") },
    ...(platformOrigin() && s.billing?.billing ? [{ label: s.billing.relay.warning ? "Plan and usage… (over the free relay)" : "Plan and usage…", run: () => openPlatform("/settings") }] : []),
  ];
}
