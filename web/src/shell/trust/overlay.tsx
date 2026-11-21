// Control's overlay over the app (M17): one thing at a time, in order of
// what needs an answer first (a passkey to confirm, a link's join or
// invite; someone asking into a team; a session offered; what control kept
// to say), else the dialog the account menu opened, else the one-time
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
import { useSubscribe } from "../../shared/ui/hooks";
import { Modal } from "../../shared/ui/modal";
import { ADD_PASSKEY_HASH, inviteInHash, JOIN_HASH, type ControlSession } from "../control";
import { SharedDocs } from "../ui/shared-docs";
import { ACCOUNT_HASH, AccountPanel, GITHUB_LINK_HASH } from "./account";
import { EMAIL_HASH } from "./email";
import { JoinPrompt } from "./approve";
import { DEPLOY_HASH, DeployPrompt } from "./deploy";
import { openPanel, useControl, usedLink, type Panel } from "./common";
import { Devices } from "./devices";
import { AddMachine } from "./machines";
import { AddPasskeyAgain, AdmitPrompt, AppLoginDone, AppLoginPrompt, ConfirmPasskey, InvitePrompt, Joined, Notice, PresignedPrompt, ShareOfferPrompt } from "./prompts";
import { Teams } from "./teams";

/** Control's address that opens Teams (where a team's owner turns chat on). */
const TEAMS_HASH = "#teams";

/** Control's addresses for the rest of the account's dialogs: the app's
 * Settings opens them here (Arugula #1108). `#account` and `#teams` are
 * above. */
const PANEL_HASH = /^#(devices|shared-docs)$/;

/** The panel open, held here rather than in the overlay: the overlay
 * mounts afresh when the page switches machine (`ControlRoot` is keyed by
 * the client's base), and a panel opened just before stays open (Arugula
 * #960). Listened for from the start, so an event sent between the two
 * mounts isn't lost either. */
let openPanelNow: null | Panel = null;
const panelListeners = new Set<() => void>();
const setOpenPanel = (p: null | Panel) => {
  openPanelNow = p;
  for (const fn of panelListeners) fn();
};
addEventListener("arugula:control-panel", (e) => setOpenPanel((e as CustomEvent<Panel>).detail));

/** Over the app: a join to add, invites, the machines list. */
export function ControlOverlay({ s }: { s: ControlSession }) {
  useControl(s);
  useSubscribe((fn) => {
    panelListeners.add(fn);
    return () => panelListeners.delete(fn);
  });
  const [hash, setHash] = useState(location.hash);
  const panel = openPanelNow;
  const setPanel = setOpenPanel;
  useEffect(() => {
    const on = () => setHash(location.hash);
    addEventListener("hashchange", on);
    // Says the overlay is on the page, so a test can wait for it before
    // opening a panel.
    document.documentElement.dataset.controlPanels = "";
    // Mounted afresh (the page switches machine when one is approved, and
    // that happens before the prompt clears the hash): catch up with a
    // hashchange that fired before this listener was there.
    on();
    return () => {
      removeEventListener("hashchange", on);
      delete document.documentElement.dataset.controlPanels;
    };
  }, []);
  if (s.phase !== "ready") return null;
  if (s.passkeyToConfirm) return <ConfirmPasskey s={s} />;
  if (hash === ADD_PASSKEY_HASH) return <AddPasskeyAgain s={s} />;
  const join = JOIN_HASH.exec(hash);
  if (join) return <JoinPrompt s={s} code={join[1]} />;
  const appLogin = /^#app=([0-9a-f]{16,128})$/.exec(hash)?.[1];
  if (appLogin) return <AppLoginPrompt s={s} id={appLogin} />;
  if (hash === "#app-done") return <AppLoginDone />;
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
  const offer = s.offers[0];
  if (offer) return <ShareOfferPrompt s={s} o={offer} />;
  if (s.joined) return <Joined s={s} />;
  const notice = s.notices[0];
  if (notice) return <Notice s={s} n={notice} />;
  // Back from an email's verify link (#1609): Sign-in and account says how
  // it went.
  const emailBack = EMAIL_HASH.exec(hash)?.[1];
  if (emailBack) {
    const done = () => {
      history.replaceState(history.state, "", location.pathname + location.search);
      setHash("");
    };
    return (
      <Modal close={done}>
        <AccountPanel s={s} close={done} emailBack={emailBack} />
      </Modal>
    );
  }
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
  // The desktop app sends Link GitHub to a browser here (#235).
  if (hash === ACCOUNT_HASH) {
    const done = () => {
      history.replaceState(history.state, "", location.pathname + location.search);
      setHash("");
    };
    return (
      <Modal close={done}>
        <AccountPanel s={s} close={done} />
      </Modal>
    );
  }
  // The platform page's Turn on chat comes here, to the teams (Discovery 5).
  if (hash === TEAMS_HASH) {
    const done = () => {
      history.replaceState(history.state, "", location.pathname + location.search);
      setHash("");
    };
    return <Teams s={s} close={done} />;
  }
  const named = PANEL_HASH.exec(hash)?.[1];
  if (named) {
    const done = () => {
      history.replaceState(history.state, "", location.pathname + location.search);
      setHash("");
    };
    if (named === "devices") return <Devices s={s} close={done} />;
    return (
      <Modal close={done}>
        <SharedDocs s={s} close={done} />
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
    ...(s.stale ? [{ header: "Arugula cloud unreachable" } as MenuItem] : []),
    { label: "Add a machine…", run: openPanel("add") },
  ];
}

/** The account's own items, in the account menu on every page (the bar's,
 * and the no-machines screen's, #97). Sign out is the menu's own, last. */
export function accountItems(s: ControlSession): MenuItem[] {
  return [
    { label: "Teams…", run: openPanel("teams") },
    { label: "Machines…", run: openPanel("devices") },
    // #1066; everyone's since Docs left Labs (native #204). Where there's
    // an app, its Docs' Shared by you (Arugula #1315), which sends it back
    // here (`#shared-docs`) while its host doesn't serve the links yet.
    { label: "Shared Docs…", run: platformOrigin() ? () => openPlatform("/docs?shared=1") : openPanel("shared-docs") },
    { label: "Sign-in and account…", run: openPanel("account") },
    ...(platformOrigin() && s.billing?.billing ? [{ label: s.billing.relay.warning ? "Plan and usage… (over the free relay)" : "Plan and usage…", run: () => openPlatform("/settings/personal") }] : []),
  ];
}
