// Settings' You scope (Arugula #1104, at `/settings`): what's about you
// (Arugula #897), four sections in a flat list (Arugula #1108, #1317,
// #1320). Notifications: this device (a ↗ row: push on this device is
// control's to set, since a device subscribes with its own keys on
// control's origin, aru-081), what every device is pushed about, drawn in
// place (./you-push.tsx, Arugula #1310), how loud chat is, words that
// notify you, and quiet hours. Appearance: this device's palette, finish,
// corners and density, the dialog's controls drawn in place, whether this
// device is lent to agents (its hand runs in the app, shared/lending.ts),
// and under them your preferences (Tips, the editor), which are the
// account's (Arugula #1318). Sign-in and security, control's dialog drawn
// in place (./you-security.tsx, Arugula #1311), with the Docs you shared
// with a link as a ↗ row under it until Docs has them (Arugula #1315).
// Connected apps and agents: the products linked to the account (M229),
// agents with no machine (M230), and the apps that may use Claude for you,
// with what your own key spent (M231). Each merged section's old address
// lands on it (paths.ts, MOVED).
//
// Elsewhere: your machines and teams are in Team and billing, as are plan
// and billing, each owner's; hosted Arugula's status page is in the avatar
// menu (bar.tsx); Help is in the command palette.

import { onAppearance, palette, appearance, FINISHES } from "../../shared/appearance";
import { lendReached, lending, onLending, setLend } from "../../shared/lending";
import { EDITORS, editorShown, setEditorPref, type Editor } from "../../shared/editor-pref";
import { bigTipsOn, setBigTips, setTips, subscribeSeen, tipsOn } from "../../shared/seen";
import { AppearanceControls, ResetAppearance } from "../../shared/ui/appearance";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import { session } from "../session";
import { AgentTokens } from "../settings/agent-tokens";
import { ChatAlerts } from "../settings/chat-alerts";
import { ClaudeSettings } from "../settings/claude";
import { LinkedProducts } from "../settings/linked";
import { ChatNotifications, QuietHoursSettings } from "../settings/notifications";
import { joined } from "../settings/summary";
import { PhonesAndBrowsers } from "./you-push";
import { SignInAndSecurity } from "./you-security";

export function YouSettings() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <>
      <Section id="notifications" title="Notifications" lede="Which devices are told, what about, and when to stay quiet.">
        <PhonesAndBrowsers />
        <ChatNotifications />
        <ChatAlerts />
        <QuietHoursSettings />
      </Section>
      <Appearance />
      <SignInAndSecurity />
      <Section
        id="connected"
        title="Connected apps and agents"
        lede="What else acts for you: products you sign in to with Arugula, agents with no machine, and apps that use Claude."
      >
        <LinkedProducts />
        <AgentTokens />
        <ClaudeSettings me={session.me?.account} />
      </Section>
    </>
  );
}

/** How Arugula looks on this device, the controls in place, then your
 * preferences, a part of it. */
function Appearance() {
  useSubscribe(onAppearance);
  useSubscribe(subscribeSeen);
  useSubscribe(onLending);
  const finish = FINISHES.find((f) => f.id === appearance().finish)?.name;
  return (
    <Section
      id="appearance"
      title="Appearance"
      summary={joined(`${palette().name}${finish ? `, ${finish}` : ""} on this device`, lending() && "lent to agents", preferencesSaid())}
      lede="Palette, finish, corners and chat density on this device, whether agents may use it, and your preferences."
    >
      <AppearanceControls />
      <div class="prompt-buttons">
        <ResetAppearance />
      </div>
      <LendDevice />
      <Preferences />
    </Section>
  );
}

/** S33: this device's tools (location, camera, mic, clipboard) for
 * agents on your machines, while this page is open: this device only. */
function LendDevice() {
  useSubscribe(onLending);
  const on = lending();
  const reached = lendReached();
  return (
    <ul class="platform-list settings-rows">
      <li>
        <label class="settings-switch">
          <input type="checkbox" role="switch" class="sw-switch" checked={on} data-lend-device onChange={(e) => setLend(e.currentTarget.checked)} />
          <span>Lend this device to agents</span>
        </label>
        <span class="dim">
          Agents on your machines may ask for its location, camera, microphone or clipboard, or ask you; you allow or deny each
          while this page is open. This device only{on ? `, reaching ${reached} machine${reached === 1 ? "" : "s"}` : ""}.
        </span>
      </li>
    </ul>
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
