// Settings' This machine (Arugula #1107): the machine's own settings, the
// ones its menu opens as dialogs (`machineSettingsItems` in ./commands),
// as sections of the app's Settings, each the dialog's own controls. The
// daemon keeps them and answers its owner alone, so they're read through a
// client to it; one that isn't connected, or isn't the owner's, draws each
// section saying why in its summary, so the list and its addresses stay.
// Who reached it through Arugula cloud (./remote-sessions) is last, and
// only where the machine answers (Arugula #1110: it was control's
// `#settings`).

import type { ComponentChildren } from "preact";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import type { Client } from "../client";
import { AccessSection } from "./access";
import { DeveloperSection } from "./developer";
import { RemoteSessionsSection } from "./remote-sessions";
import { ReportsSection } from "./machine-telemetry";
import { RulesSection } from "./rules";
import { SharingSection } from "./sharing";

/** The machine's own sections, in Settings' order: Access first, then
 * what the page puts after it (`children`), then the rest. */
export const MACHINE_SECTIONS = [
  { id: "access", title: "Access" },
  { id: "rules", title: "Permission rules" },
  { id: "reports", title: "Usage and crash reports" },
  { id: "guest-ssh", title: "Guest SSH access" },
  { id: "developer", title: "Developer settings" },
] as const;

/** Why the sections can't show, in a few words for the list and a line
 * under each heading; `out`, where else to go for them (a row to the
 * machine's page). */
export interface Away {
  summary: string;
  line: ComponentChildren;
  out?: ComponentChildren;
}

/** Each section, or why not: `away`, when it's known the client won't do. */
export function MachineSections({ client, away, children }: { client: Client | null; away?: Away; children?: ComponentChildren }) {
  useSubscribe((fn) => client?.subscribe(fn) ?? (() => {}));
  const why: Away | undefined =
    away ??
    (!client?.connected || !client.state
      ? { summary: "Reaching it…", line: "Reaching it…" }
      : client.state.roles
        ? { summary: "Its owner's", line: "Only its owner sees and changes these." }
        : undefined);
  if (why) {
    const [first, ...rest] = MACHINE_SECTIONS;
    const stand = (s: (typeof MACHINE_SECTIONS)[number]) => (
      <Section key={s.id} id={s.id} title={s.title} summary={why.summary} hooks={{ "data-machine-away": "" }}>
        <p class="dim">{why.line}</p>
        {why.out}
      </Section>
    );
    return (
      <>
        {stand(first)}
        {children}
        {rest.map(stand)}
      </>
    );
  }
  const c = client!;
  return (
    <>
      <AccessSection client={c} />
      {children}
      <RulesSection client={c} />
      <ReportsSection client={c} />
      <SharingSection client={c} />
      <DeveloperSection client={c} />
      <RemoteSessionsSection client={c} />
    </>
  );
}
