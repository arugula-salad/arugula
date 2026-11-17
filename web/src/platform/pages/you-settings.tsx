// Settings' You scope (Arugula #1104, at `/settings`): what's about you
// (Arugula #897), in three bands (Arugula #1108). Notifications: how loud
// chat is and quiet hours, words that notify you, and which of your phones
// and browsers are pushed what. Look and feel: Appearance (this device's,
// the shell's dialog; the one place it's in) and your preferences (Tips,
// the editor), which are the account's. Account: what a device signs
// (sign-in, passkeys, devices, teams, shared Docs), as ↗ rows that open
// control's page at that dialog; the products linked to the account (M229);
// which machines' agents read and post (M197); agents with no machine
// (M230); and the apps that may use Claude for you, with what your own key
// spent (M231). Plan and billing are each owner's, in Team and billing.
//
// Push is control's to set: a device subscribes with its own keys, and the
// hub answers push settings only there. Help isn't a section: it's in the
// command palette.

import { onAppearance, palette, appearance, FINISHES } from "../../shared/appearance";
import { EDITORS, editorShown, setEditorPref, type Editor } from "../../shared/editor-pref";
import { bigTipsOn, setBigTips, setTips, subscribeSeen, tipsOn } from "../../shared/seen";
import { openAppearance } from "../../shared/ui/appearance";
import { ExternalIcon } from "../../shared/ui/icons";
import { useSubscribe } from "../../shared/ui/hooks";
import { Band, Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { owners } from "../owners";
import { follow, pathOf } from "../router";
import { controlOrigin, session } from "../session";
import { AgentGrants } from "../settings/agents";
import { AgentTokens } from "../settings/agent-tokens";
import { ChatAlerts } from "../settings/chat-alerts";
import { ClaudeSettings } from "../settings/claude";
import { LinkedProducts } from "../settings/linked";
import { NotificationSettings } from "../settings/notifications";
import { joined } from "../settings/summary";

export function YouSettings() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <>
      <Band title="Notifications">
        <NotificationSettings />
        <ChatAlerts />
        <PhonesAndBrowsers />
      </Band>
      <Band title="Look and feel">
        <Appearance />
        <Preferences />
      </Band>
      <Band title="Account">
        <Account />
        <LinkedProducts />
        <AgentGrants />
        <AgentTokens />
        <ClaudeSettings me={session.me?.account} />
      </Band>
    </>
  );
}

/** A row a device signs (Arugula #1108): it opens control's page at its
 * dialog, in this tab. */
function OnControl({ hash, label, hint }: { hash: string; label: string; hint: string }) {
  return (
    <li>
      <a class="settings-open" href={`${controlOrigin()}/#${hash}`} title="Opens your machines page" data-settings-control={hash}>
        <span class="settings-open-name">
          {label} <ExternalIcon size="xs" />
        </span>
        <span class="dim">{hint}</span>
      </a>
    </li>
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
        <OnControl hash="settings=push" label="Notify this device" hint="Turn this browser or phone on or off" />
        <OnControl hash="settings=push" label="What every device is pushed about" hint="Chat, your machines, your projects; and a test notification" />
      </ul>
    </Section>
  );
}

/** How Arugula looks on this device: the shell's dialog. */
function Appearance() {
  useSubscribe(onAppearance);
  const finish = FINISHES.find((f) => f.id === appearance().finish)?.name;
  return (
    <Section id="appearance" title="Appearance" summary={`${palette().name}${finish ? `, ${finish}` : ""} on this device`} lede="Palette, finish, corners and chat density. This device's only.">
      <ul class="platform-list settings-rows">
        <li>
          <button class="settings-open" data-open-appearance onClick={openAppearance}>
            <span class="settings-open-name">Appearance…</span>
          </button>
        </li>
      </ul>
    </Section>
  );
}

/** Tips and what Open in editor opens: your account's, so they follow you
 * to every device (shared/seen.ts), as control's page has them. */
function Preferences() {
  useSubscribe(subscribeSeen);
  const editor = editorShown();
  const tips = tipsOn() ? (bigTipsOn() ? "Tips on" : "Tips on, not big ones") : "Tips off";
  const opens = editor === "ask" ? "asks which editor" : `opens in ${EDITORS.find(([e]) => e === editor)?.[1]}`;
  return (
    <Section id="preferences" title="Preferences" summary={joined(tips, opens)} lede="These follow your account to every device." hooks={{ "data-preferences": "" }}>
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

/** What a device signs: sign-in, passkeys, devices, teams and the Docs
 * shared with you, each a ↗ row to its dialog on control's page. */
function Account() {
  // Plan and billing are each owner's, in Team and billing.
  const plans = owners(session.projects).map((o) => ({ ...o, href: pathOf({ page: "settings", scope: "team", owner: o.key, section: "plan" }, names) }));
  return (
    <Section
      id="account"
      title="Sign-in and devices"
      summary="On your machines page"
      lede="One of your devices signs these, so each opens your machines page at it."
      hooks={{ "data-settings-account": "" }}
    >
      <ul class="platform-list settings-rows">
        <OnControl hash="account" label="Sign-in and account" hint="Passkeys, GitHub, email and where you're signed in" />
        <OnControl hash="devices" label="Devices and machines" hint="What you've approved, and taking one back" />
        <OnControl hash="add-device" label="Add a phone or browser" hint="Approve a new one from this one" />
        <OnControl hash="teams" label="Teams" hint="The teams you're in, and inviting people" />
        <OnControl hash="shared-docs" label="Shared Docs" hint="Docs people shared with you" />
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
