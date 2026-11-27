// The Settings page on a daemon's own page and control's (Arugula #695):
// `#settings`. Its rows are the ones the menus and Go to… use
// (./commands), drawn as a page: a toggle is a switch, a hint is said out
// loud. Where the app's Settings is, this page keeps only Phones and
// browsers, which a device turns on with its own keys, and its other
// sections' old links land in the app (Arugula #1110, ./app.tsx). A
// daemon's own page has no app, so it keeps every section. Help is the
// command palette's, not a section here. Appearance draws the dialog's
// controls in place, with Preferences a part of it (Arugula #1318), so
// `#settings=preferences` opens at Appearance.

import { useState } from "preact/hooks";
import type { Client } from "../client";
import { platformHref, openPlatform } from "../platform-links";
import { subscribeSeen } from "../../shared/seen";
import { useSubscribe } from "../../shared/ui/hooks";
import type { MenuItem } from "../../shared/ui/menu";
import { SettingsList } from "../../shared/ui/settings-list";
import { Section } from "../../shared/ui/settings-section";
import { accountHeader, AppBar, useAccount } from "./app-bar";
import { settingsGroups } from "./commands";
import { RemoteSessionsSection } from "./remote-sessions";
import { subscribePush } from "./notify";
import { settingsRoute } from "./settings-route";

/** One row: a switch, a choice, an action, or a line that only says. */
function MenuLine({ item, group, did }: { item: MenuItem; group?: string; did: () => void }) {
  if (item === "separator" || "header" in item) return null;
  if ("submenu" in item) {
    const choice = item.submenu.some((i) => i !== "separator" && "checked" in i && i.checked !== undefined);
    return (
      <li class="settings-row settings-row-group" data-setting={item.label}>
        <span class="settings-row-name">{item.label}</span>
        <ul class={choice ? "settings-choices" : "settings-rows"} role={choice ? "radiogroup" : undefined} aria-label={item.label}>
          {item.submenu.map((i, n) => (
            <MenuLine key={n} item={i} group={choice ? item.label : undefined} did={did} />
          ))}
        </ul>
      </li>
    );
  }
  // A row's run may change what the rows say, and not every one tells a store.
  const run = () => {
    item.run();
    did();
  };
  const hint = item.hint && item.hint !== item.label ? <span class="dim">{item.hint}</span> : null;
  if (group) {
    // On click, not change: a default that's shown but not yet chosen (Editor's
    // VS Code) is chosen by clicking it, as its row in a menu is.
    return (
      <li>
        <label class="settings-choice">
          <input type="radio" name={`setting-${group}`} checked={!!item.checked} onClick={run} /> {item.label}
        </label>
      </li>
    );
  }
  if (item.checked !== undefined) {
    return (
      <li class="settings-row" data-setting={item.label}>
        <label class="settings-switch">
          <input type="checkbox" role="switch" class="sw-switch" checked={item.checked} disabled={item.disabled} onChange={run} /> {item.label}
        </label>
        {hint}
      </li>
    );
  }
  if (item.disabled) {
    return (
      <li class="settings-row dim" data-setting-note>
        {item.label}
      </li>
    );
  }
  return (
    <li class="settings-row" data-setting={item.label}>
      <button class="settings-open" aria-label={item.label} onClick={run}>
        <span class="settings-open-name">{item.label}</span>
        {hint}
      </button>
    </li>
  );
}

export function SettingsPage({ client }: { client: Client }) {
  useSubscribe((fn) => client.subscribe(fn));
  useSubscribe(subscribePush);
  useSubscribe(subscribeSeen);
  const account = useAccount();
  const [, tick] = useState(0);
  const did = () => tick((t) => t + 1);
  const groups = settingsGroups(client);
  const rows = (items: MenuItem[]) => (
    <ul class="settings-rows">
      {items.map((item, n) => (
        <MenuLine key={n} item={item} did={did} />
      ))}
    </ul>
  );
  const at = (group: string) => groups.find((g) => g.id === group);
  const section = (id: string) => {
    const g = at(id);
    return g ? (
      <Section key={id} id={id} title={g.title}>
        {rows(g.items)}
      </Section>
    ) : null;
  };
  // Look took Appearance (Lift 7): its old address opens Preferences.
  const routed = settingsRoute()?.section;
  const open = routed === "appearance" ? "preferences" : routed;
  const hrefOf = (id: string | null) => (id ? `#settings=${id}` : "#settings");
  const go = (href: string) => {
    if (location.hash !== href) location.hash = href;
  };
  // The app's Settings, where there is one: this page is Phones and browsers.
  const app = platformHref("/settings");
  return (
    <div class="chat agents-page settings-place" data-settings-page>
      <AppBar client={client} at="settings" />
      <div class="settings-place-scroll">
        <SettingsList label="Settings" header={<h1>Settings</h1>} section={open} hrefOf={hrefOf} go={go} {...(app ? { "data-settings-kept": "" } : {})}>
          {section("push")}
          {app ? (
            <p class="m-meta" data-settings-in-app>
              Everything else is in{" "}
              <a
                href={app}
                onClick={(e) => {
                  if (e.metaKey || e.ctrlKey || e.shiftKey) return;
                  e.preventDefault();
                  openPlatform("/settings");
                }}
              >
                Settings
              </a>
              .
            </p>
          ) : (
            <>
              {section("preferences")}
              {section("machine")}
              <RemoteSessionsSection client={client} />
              {account && (
                <Section id="account" title="Account" summary={accountHeader(account.login())}>
                  {rows(account.items())}
                </Section>
              )}
            </>
          )}
        </SettingsList>
      </div>
    </div>
  );
}
