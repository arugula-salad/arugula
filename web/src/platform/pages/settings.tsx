// `/settings`: the account's platform settings (M150), only what's about
// you (Arugula #897): which machines' agents read and post in conversations
// (M197), agents with no machine (M230), the products linked to the
// account (M229), and which apps may use Claude through Arugula for the
// account, with what was spent on its own key (M231), and this device's
// Appearance…, the shell's dialog. Plan and billing are each owner's, on
// its settings (./owner-settings): Personal's and each team's. Sign-in,
// passkeys, devices and teams are trust, signed by a device, so they stay
// on control's page and this links there. Each part is a section
// (shared/ui/settings-section): a heading, one line, the rest behind More;
// on a wide screen an index of them sits beside the column
// (../settings.css).

import { useRef } from "preact/hooks";
import { openAppearance } from "../../shared/ui/appearance";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { AgentGrants } from "../settings/agents";
import { AgentTokens } from "../settings/agent-tokens";
import { LinkedProducts } from "../settings/linked";
import { ChatAlerts } from "../settings/chat-alerts";
import { ClaudeSettings } from "../settings/claude";
import { NotificationSettings } from "../settings/notifications";
import { ownerOf } from "../settings/overrides";
import { SHELL_SECTIONS } from "../../shared/settings-map";
import { ElsewhereLinks, Section, SectionIndex, type Elsewhere } from "../../shared/ui/settings-section";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { controlOrigin, session } from "../session";

export function SettingsPage() {
  useSubscribe((fn) => session.subscribe(fn));
  const body = useRef<HTMLDivElement>(null);
  // The rest of Settings is on your machines page (Arugula #695).
  const elsewhere: Elsewhere = {
    title: "On your machines page",
    links: SHELL_SECTIONS.map((s) => ({ title: s.title, href: `${controlOrigin()}/#settings=${s.id}` })),
  };
  // Each owner's settings: Personal's, then each team's.
  const teams = new Map(session.projects.map(ownerOf).filter((o) => o.owner !== "personal").map((o) => [o.owner, o.name]));
  const owners = [{ owner: "personal", name: "Personal" }, ...[...teams].map(([owner, name]) => ({ owner, name }))].map((o) => ({
    ...o,
    href: pathOf({ page: "owner-settings", owner: o.owner }, names),
  }));
  return (
    <section class="platform-section sw-page settings-page" data-platform-settings>
      <div class="settings-layout">
        <SectionIndex body={body} elsewhere={elsewhere} />
        <div class="settings-body" ref={body}>
          <PageHeader title="Settings" />
          <Section id="account" title="Appearance & account">
            <ul class="platform-list settings-rows">
              <li>
                <button class="settings-open" data-open-appearance onClick={openAppearance}>
                  <span class="settings-open-name">Appearance…</span>
                </button>
              </li>
              <li>
                <a class="settings-open" href={`${controlOrigin()}/#settings=account`} data-settings-control>
                  <span class="settings-open-name">Sign-in, passkeys, devices, teams and usage reports</span>
                </a>
              </li>
            </ul>
            <p class="dim" data-plan-moved>
              Plan and billing are in each owner's settings:{" "}
              {owners.map((o, i) => (
                <span key={o.owner}>
                  {i > 0 && " · "}
                  <a href={o.href} data-owner-settings-link={o.owner} onClick={(e) => follow(e, o.href)}>
                    {o.name}
                  </a>
                </span>
              ))}
              .
            </p>
          </Section>
          <NotificationSettings />
          <LinkedProducts />
          <AgentGrants />
          <AgentTokens />
          <ChatAlerts />
          <ClaudeSettings me={session.me?.account} />
          <ElsewhereLinks elsewhere={elsewhere} />
        </div>
      </div>
    </section>
  );
}
