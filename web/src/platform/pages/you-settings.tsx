// Settings' You scope (Arugula #1104, at `/settings`): what's about you
// (Arugula #897), as `/settings` had it (M150): Appearance… (this device's,
// the shell's dialog) and the way to sign-in, passkeys, devices and teams,
// which a device signs and so stay on control's page; how loud chat is and
// quiet hours; the products linked to the account (M229); which machines'
// agents read and post (M197); agents with no machine (M230); words that
// notify you; and the apps that may use Claude for you, with what your own
// key spent (M231). Plan and billing are each owner's, in Team and billing.
// What's still only on control's page is a section of links to it, until
// it moves in (Arugula #1108).

import { onAppearance, palette, appearance, FINISHES } from "../../shared/appearance";
import { openAppearance } from "../../shared/ui/appearance";
import { ExternalIcon } from "../../shared/ui/icons";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import { SHELL_SECTIONS } from "../../shared/settings-map";
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

export function YouSettings() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <>
      <Account />
      <NotificationSettings />
      <LinkedProducts />
      <AgentGrants />
      <AgentTokens />
      <ChatAlerts />
      <ClaudeSettings me={session.me?.account} />
      <Elsewhere />
    </>
  );
}

/** Appearance & account: this device's look, and the way to what a device signs. */
function Account() {
  useSubscribe(onAppearance);
  const finish = FINISHES.find((f) => f.id === appearance().finish)?.name;
  // Plan and billing are each owner's, in Team and billing.
  const plans = owners(session.projects).map((o) => ({ ...o, href: pathOf({ page: "settings", scope: "team", owner: o.key, section: "plan" }, names) }));
  return (
    <Section id="account" title="Appearance & account" summary={`${palette().name}${finish ? `, ${finish}` : ""} on this device`}>
      <ul class="platform-list settings-rows">
        <li>
          <button class="settings-open" data-open-appearance onClick={openAppearance}>
            <span class="settings-open-name">Appearance…</span>
          </button>
        </li>
        <li>
          <a class="settings-open" href={`${controlOrigin()}/#settings=account`} data-settings-control>
            <span class="settings-open-name">
              Sign-in, passkeys, devices, teams and usage reports <ExternalIcon size="xs" />
            </span>
          </a>
        </li>
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

/** The rest of Settings, on your machines page (Arugula #695) until it
 * moves in. */
function Elsewhere() {
  return (
    <Section id="elsewhere" title="On your machines page" summary={SHELL_SECTIONS.map((s) => s.title).join(", ")} lede="These are on control's page for now: each opens it there.">
      <ul class="platform-list settings-rows" data-settings-elsewhere>
        {SHELL_SECTIONS.map((s) => (
          <li key={s.id}>
            <a class="settings-open" href={`${controlOrigin()}/#settings=${s.id}`} data-settings-control>
              <span class="settings-open-name">
                {s.title} <ExternalIcon size="xs" />
              </span>
            </a>
          </li>
        ))}
      </ul>
    </Section>
  );
}
