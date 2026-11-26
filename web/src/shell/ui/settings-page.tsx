// The Settings page on a daemon's own page and control's (Arugula #695):
// `#settings`. Its rows are the ones the menus and the palette use
// (./commands), drawn as a page: a toggle is a switch, a hint is said out
// loud. Where the app's Settings is, this page keeps only Phones and
// browsers, which a device turns on with its own keys, and its other
// sections' old links land in the app (Arugula #1110, ./app.tsx). A
// daemon's own page has no app, so it keeps every section. Help is the
// command palette's, not a section here. Appearance draws the dialog's
// controls in place, with Preferences a part of it (Arugula #1318), so
// `#settings=preferences` opens at Appearance.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { platformHref, openPlatform } from "../platform-links";
import { subscribeSeen } from "../../shared/seen";
import { useSubscribe } from "../../shared/ui/hooks";
import type { MenuItem } from "../../shared/ui/menu";
import { AppearanceControls, ResetAppearance } from "../../shared/ui/appearance";
import { PageHeader } from "../../shared/ui/page-header";
import { Section, SectionIndex } from "../../shared/ui/settings-section";
import { accountHeader, AppBar, useAccount } from "./app-bar";
import { settingsGroups } from "./commands";
import { RemoteSessionsSection } from "./remote-sessions";
import { subscribePush } from "./notify";
import { settingsRoute } from "./settings-route";

/** One row: a switch, a choice, an action, or a line that only says. */
function Row({ item, group, did }: { item: MenuItem; group?: string; did: () => void }) {
  if (item === "separator" || "header" in item) return null;
  if ("submenu" in item) {
    const choice = item.submenu.some((i) => i !== "separator" && "checked" in i && i.checked !== undefined);
    return (
      <li class="settings-row settings-row-group" data-setting={item.label}>
        <span class="settings-row-name">{item.label}</span>
        <ul class={choice ? "settings-choices" : "settings-rows"} role={choice ? "radiogroup" : undefined} aria-label={item.label}>
          {item.submenu.map((i, n) => (
            <Row key={n} item={i} group={choice ? item.label : undefined} did={did} />
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
  const body = useRef<HTMLDivElement>(null);
  const [, tick] = useState(0);
  const did = () => tick((t) => t + 1);
  const groups = settingsGroups(client);
  const rows = (items: MenuItem[]) => (
    <ul class="settings-rows">
      {items.map((item, n) => (
        <Row key={n} item={item} did={did} />
      ))}
    </ul>
  );
  const at = (group: string) => groups.find((g) => g.id === group);
  const section = (id: string) => {
    const g = at(id);
    return g ? (
      <Section key={id} id={id} title={g.title} lede={g.lede}>
        {rows(g.items)}
      </Section>
    ) : null;
  };
  // The route's section, once: the hash stays `#settings=…` as you scroll.
  // Preferences is Appearance's now.
  const routed = () => {
    const s = settingsRoute()?.section;
    return s === "preferences" ? "appearance" : s;
  };
  const opened = useRef(routed());
  useEffect(() => {
    const fn = () => (opened.current = routed());
    addEventListener("hashchange", fn);
    return () => removeEventListener("hashchange", fn);
  }, []);
  // The app's Settings, where there is one: this page is Phones and browsers.
  const app = platformHref("/settings");
  return (
    <div class="chat agents-page settings-place" data-settings-page>
      <AppBar client={client} at="settings" />
      <div class="settings-place-scroll">
        {app ? (
          <div class="settings-body" data-settings-kept>
            <PageHeader title="Settings" />
            {section("push")}
            <p class="dim" data-settings-in-app>
              Your other settings, and those of your machines, projects and team, are in{" "}
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
          </div>
        ) : (
          <div class="settings-layout">
            <SectionIndex body={body} anchor={(id) => `#settings=${id}`} opened={() => opened.current} />
            <div class="settings-body" ref={body}>
              <PageHeader title="Settings" />
              {section("push")}
              <Section id="appearance" title="Appearance" lede="Theme and look on this device, and your preferences.">
                <AppearanceControls />
                <div class="prompt-buttons">
                  <ResetAppearance />
                </div>
                {section("preferences")}
              </Section>
              {section("machine")}
              <RemoteSessionsSection client={client} />
              {account && (
                <Section id="account" title="Account" lede={accountHeader(account.login())}>
                  {rows(account.items())}
                </Section>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
