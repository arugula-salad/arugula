// Settings' You scope (Arugula #1104, at `/settings`): what's about you
// (Arugula #897), in a flat list (Arugula #1108, #1317). First how loud
// chat is and quiet hours, words that notify you, and which of your phones
// and browsers are pushed what. Then Appearance: this device's palette,
// finish, corners and density, the dialog's controls drawn in place, and
// under them your preferences (Tips, the editor), which are the account's
// (Arugula #1318; `preferences`' old address lands here). Then Sign-in and
// security, control's dialog drawn in place (./you-security.tsx, Arugula
// #1311; `account`'s old address lands there), and what's still only on
// control's page (machines, teams, shared Docs), as ↗ rows that open it at
// that dialog; the products linked to the account (M229);
// which machines' agents read and post (M197); agents with no machine
// (M230); and the apps that may use Claude for you, with what your own key
// spent (M231). Plan and billing are each owner's, in Team and billing.
// On hosted Arugula, its status page too (Arugula #1176).
//
// Push is control's to set: a device subscribes with its own keys, and the
// hub answers push settings only there. Help isn't a section: it's in the
// command palette.

import { onAppearance, palette, appearance, FINISHES } from "../../shared/appearance";
import { EDITORS, editorShown, setEditorPref, type Editor } from "../../shared/editor-pref";
import { bigTipsOn, setBigTips, setTips, subscribeSeen, tipsOn } from "../../shared/seen";
import { AppearanceControls, ResetAppearance } from "../../shared/ui/appearance";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { owners } from "../owners";
import { follow, pathOf } from "../router";
import { controlOrigin, session } from "../session";
import { AgentTokens } from "../settings/agent-tokens";
import { ChatAlerts } from "../settings/chat-alerts";
import { ClaudeSettings } from "../settings/claude";
import { LinkedProducts } from "../settings/linked";
import { NotificationSettings } from "../settings/notifications";
import { ControlRow } from "../settings/owner-machines";
import { statusPage } from "../status-page";
import { ExternalIcon } from "../../shared/ui/icons";
import { joined } from "../settings/summary";
import { SignInAndSecurity } from "./you-security";

export function YouSettings() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <>
      <NotificationSettings />
      <ChatAlerts />
      <PhonesAndBrowsers />
      <Appearance />
      <SignInAndSecurity />
      <MachinesAndTeams />
      <Status />
      <LinkedProducts />
      <AgentTokens />
      <ClaudeSettings me={session.me?.account} />
    </>
  );
}

/** Whether this device is pushed, and what every device you turned on is
 * pushed about: control's, where the device's keys are. */
function PhonesAndBrowsers() {
  return (
    <Section
      id="push"
      title="Phones and browsers"
      summary="On your machines page"
      lede="Which of your devices are notified, and what about."
      more={<p>Each device turns itself on with its own keys, so these open your machines page.</p>}
    >
      <ul class="platform-list settings-rows">
        <ControlRow href={`${controlOrigin()}/#settings=push`} label="Notify this device" why="Turn this browser or phone on or off" hook="push" />
        <ControlRow href={`${controlOrigin()}/#settings=push`} label="What every device is pushed about" why="Chat, your machines, your projects; and a test notification" hook="push" />
      </ul>
    </Section>
  );
}

/** How Arugula looks on this device, the controls in place, then your
 * preferences, a part of it. */
function Appearance() {
  useSubscribe(onAppearance);
  useSubscribe(subscribeSeen);
  const finish = FINISHES.find((f) => f.id === appearance().finish)?.name;
  return (
    <Section
      id="appearance"
      title="Appearance"
      summary={joined(`${palette().name}${finish ? `, ${finish}` : ""} on this device`, preferencesSaid())}
      lede="Palette, finish, corners and chat density on this device, and your preferences."
    >
      <AppearanceControls />
      <div class="prompt-buttons">
        <ResetAppearance />
      </div>
      <Preferences />
    </Section>
  );
}

/** What Preferences has set, in a few words. */
function preferencesSaid(): string {
  const editor = editorShown();
  const tips = tipsOn() ? (bigTipsOn() ? "Tips on" : "Tips on, not big ones") : "Tips off";
  const opens = editor === "ask" ? "asks which editor" : `opens in ${EDITORS.find(([e]) => e === editor)?.[1]}`;
  return joined(tips, opens);
}

/** Tips and what Open in editor opens: your account's, so they follow you
 * to every device (shared/seen.ts), as control's page has them. */
function Preferences() {
  useSubscribe(subscribeSeen);
  const editor = editorShown();
  return (
    <Section id="preferences" title="Preferences" summary={preferencesSaid()} lede="These follow your account to every device." hooks={{ "data-preferences": "" }}>
      <ul class="platform-list settings-rows">
        <li>
          <label class="settings-switch">
            <input type="checkbox" role="switch" class="sw-switch" checked={tipsOn()} data-pref="tips" onChange={(e) => setTips(e.currentTarget.checked)} />
            <span>Tips</span>
          </label>
          <span class="dim">A one-time mention when something you can use turns up, like a port listening in a pane</span>
        </li>
        <li>
          <label class="settings-switch">
            <input type="checkbox" role="switch" class="sw-switch" checked={bigTipsOn()} data-pref="big-tips" onChange={(e) => setBigTips(e.currentTarget.checked)} />
            <span>Big tips</span>
          </label>
          <span class="dim">Tips that cover a pane until you close them, like a feature's help card</span>
        </li>
        <li>
          <label class="settings-field">
            Open in editor
            <select class="field" value={editor} data-pref="editor" onChange={(e) => setEditorPref(e.currentTarget.value as Editor)}>
              {EDITORS.map(([e, label]) => (
                <option key={e} value={e}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </li>
      </ul>
    </Section>
  );
}

/** What's still only on control's page: your machines, teams and the Docs
 * shared with you, each a ↗ row to its dialog there. Adding a phone or
 * browser went with device approvals (#1004); sign-in is in place, in
 * Sign-in and security. */
function MachinesAndTeams() {
  // Plan and billing are each owner's, in Team and billing.
  const plans = owners(session.projects).map((o) => ({ ...o, href: pathOf({ page: "settings", scope: "team", owner: o.key, section: "plan" }, names) }));
  return (
    <Section
      id="machines-teams"
      title="Machines and teams"
      summary="On your machines page"
      lede="These are on your machines page, so each opens it at that dialog."
      hooks={{ "data-settings-account": "" }}
    >
      <ul class="platform-list settings-rows">
        <ControlRow href={`${controlOrigin()}/#devices`} label="Machines" why="Your machines, and removing one" hook="devices" />
        <ControlRow href={`${controlOrigin()}/#teams`} label="Teams" why="The teams you're in, and inviting people" hook="teams" />
        <ControlRow href={`${controlOrigin()}/#shared-docs`} label="Shared Docs" why="Docs people shared with you" hook="shared-docs" />
      </ul>
      <p class="dim" data-plan-moved>
        Plan and billing are in each owner's settings:{" "}
        {plans.map((o, i) => (
          <span key={o.key}>
            {i > 0 && " · "}
            <a href={o.href} data-owner-settings-link={o.key} onClick={(e) => follow(e, o.href)}>
              {o.name}
            </a>
          </span>
        ))}
        .
      </p>
    </Section>
  );
}

/** Whether hosted Arugula is up: its status page, in a new tab. Only on
 * hosted Arugula's page (status-page.ts); a control you run yourself has
 * none of ours. Its id isn't `status`: that's the shell's status pill's
 * (style.css `#status`). */
function Status() {
  const url = statusPage(controlOrigin());
  if (!url) return null;
  return (
    <Section id="status-page" title="Status" summary={new URL(url).host} lede="Whether hosted Arugula is up right now, and any incident we're working on." hooks={{ "data-settings-status": "" }}>
      <ul class="platform-list settings-rows">
        <li>
          <a class="settings-open" href={url} target="_blank" rel="noopener" data-status-page>
            <span class="settings-open-name">
              Status <ExternalIcon size="xs" />
            </span>
            <span class="dim">Control, the platform, installs and downloads</span>
          </a>
        </li>
      </ul>
    </Section>
  );
}
