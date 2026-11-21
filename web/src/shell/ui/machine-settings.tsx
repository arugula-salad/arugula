// Settings' Machines (Arugula #1107): the machine's own settings, the
// ones its menu opens as dialogs (`machineSettingsItems` in ./commands),
// as parts of the app's Settings, each the dialog's own controls. The
// daemon keeps them and answers its owner alone, so they're read through a
// client to it; one that isn't connected, or isn't the owner's, draws each
// section saying why in its summary, so the list and its addresses stay.
//
// Four sections, in two bands (the Settings Doc's Machines tab): Access
// and sharing (who reaches it, Guest SSH access, Remote typing, read-only,
// and who reached it through Arugula cloud, ./remote-sessions), Agents
// (what they may do on Arugula cloud, the page's, and its permission
// rules), then the page's Name, and Advanced (usage and crash reports,
// developer settings, and removing arugula.nvim, which only the machine
// menu had). The old
// sections' addresses land on the one each is part of now (`MOVED` in
// platform/paths).

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { useSubscribe } from "../../shared/ui/hooks";
import { Band, Section } from "../../shared/ui/settings-section";
import type { Client } from "../client";
import { AccessSection } from "./access";
import { removePluginSaid } from "../hints/nvim-remove";
import { DeveloperSection } from "./developer";
import { RemoteSessionsSection, RemoteTypingSection } from "./remote-sessions";
import { ReportsSection } from "./machine-telemetry";
import { RulesSection } from "./rules";
import { SharingSection } from "./sharing";

/** The machine's own sections, in Settings' order. Name, the page's, goes
 * between Agents and Advanced. */
export const MACHINE_SECTIONS = {
  access: { title: "Access and sharing", lede: "Who can reach it, how, and who did." },
  agents: { title: "Agents", lede: "What its agents may do on Arugula cloud, and on this machine without asking." },
  advanced: { title: "Advanced", lede: "Its usage and crash reports, developer settings, and clean-up." },
} as const;

/** Why the sections can't show, in a few words for the list and a line
 * under each heading; `out`, where else to go for them (a row to the
 * machine's page). */
export interface Away {
  summary: string;
  line: ComponentChildren;
  out?: ComponentChildren;
}

type Id = keyof typeof MACHINE_SECTIONS;

/** One of the sections, with what it holds. */
function Sec({ id, summary, hooks, children }: { id: Id; summary?: string; hooks?: Record<`data-${string}`, string>; children: ComponentChildren }) {
  const s = MACHINE_SECTIONS[id];
  return (
    <Section id={id} title={s.title} summary={summary} lede={s.lede} hooks={hooks}>
      {children}
    </Section>
  );
}

/** Each section, or why not: `away`, when it's known the client won't do.
 * `grants`, the page's part of Agents; `children`, its sections about the
 * machine (its Name), listed before Advanced. */
export function MachineSections({ client, away, grants, children }: { client: Client | null; away?: Away; grants?: ComponentChildren; children?: ComponentChildren }) {
  useSubscribe((fn) => client?.subscribe(fn) ?? (() => {}));
  const why: Away | undefined =
    away ??
    (!client?.connected || !client.state
      ? { summary: "Reaching it…", line: "Reaching it…" }
      : client.state.roles
        ? { summary: "Its owner's", line: "Only its owner sees and changes these." }
        : undefined);
  const c = client!;
  const said = (
    <>
      <p class="dim">{why?.line}</p>
      {why?.out}
    </>
  );
  const stand = { "data-machine-away": "" } as const;
  return (
    <>
      <Band title="Who and what">
        {why ? (
          <Sec id="access" summary={why.summary} hooks={stand}>
            {said}
          </Sec>
        ) : (
          <Sec id="access">
            <AccessSection client={c} />
            <SharingSection client={c} />
            <RemoteTypingSection client={c} />
            <RemoteSessionsSection client={c} typing={!c.features || c.features.remote_input === undefined} />
          </Sec>
        )}
        <Sec id="agents">
          {grants}
          {why ? said : <RulesSection client={c} />}
        </Sec>
      </Band>
      <Band title="About it">
        {children}
        {why ? (
          <Sec id="advanced" summary={why.summary} hooks={stand}>
            {said}
          </Sec>
        ) : (
          <Sec id="advanced">
            <ReportsSection client={c} />
            <DeveloperSection client={c} />
            <NvimSection client={c} />
          </Sec>
        )}
      </Band>
    </>
  );
}

/** Remove arugula.nvim (#1733), which was the machine menu's alone: the
 * copies Arugula installed on the machine go, one a plugin manager
 * installed stays. */
function NvimSection({ client }: { client: Client }) {
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; said: string } | null>(null);
  return (
    <Section id="nvim" title="Remove arugula.nvim" lede="Deletes the copies of arugula.nvim that Arugula installed on this machine. One your plugin manager installed stays.">
      <div class="prompt-buttons">
        <button
          type="button"
          class="secondary"
          disabled={busy}
          data-nvim-remove
          onClick={() => {
            setBusy(true);
            setSaid(null);
            void removePluginSaid(client).then((r) => {
              setBusy(false);
              setSaid(r);
            });
          }}
        >
          Remove
        </button>
      </div>
      {said && (
        <p class={said.ok ? "dim" : "error"} data-nvim-removed>
          {said.said}
        </p>
      )}
    </Section>
  );
}
