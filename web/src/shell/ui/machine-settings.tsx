// Settings › Machines (Arugula #1107): the machine's own settings, the
// ones its menu opens as dialogs (`machineSettingsItems` in ./commands),
// as parts of the app's Settings, each the dialog's own controls. The
// daemon keeps them and answers its owner alone, so they're read through a
// client to it. Where that client won't do (the machine is offline, isn't
// answering, can't be reached from here, or isn't yours), each section
// still stands, so the list and its addresses stay: its row says why, and
// over its parts, dimmed, one note says why and what to do. What Arugula
// cloud keeps (its agents' grants, its name) works either way.
//
// Four sections, in two bands (the Settings Doc's Machines tab): Access
// and sharing (who reaches it, Guest SSH access, Remote typing, read-only,
// and who reached it through Arugula cloud, ./remote-sessions), Agents
// (what they may do on Arugula cloud, the page's, and its permission
// rules), then the page's Name, and Advanced (usage and crash reports,
// developer settings, and removing arugula.nvim, which only the machine
// menu had). The old sections' addresses land on the one each is part of
// now (`MOVED` in platform/paths).

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

/** Why the machine's parts can't show: in a few words for the list, and
 * the note over them, which says what to do where there's something. */
export interface Away {
  summary: string;
  line: ComponentChildren;
  /** Only a wait (it's being reached): said quietly, not as a note. */
  wait?: boolean;
}

type Id = keyof typeof MACHINE_SECTIONS;

/** Each section's parts that are the machine's, by title, as they stand
 * dimmed while it can't be reached. */
const PARTS: Record<Id, { id: string; title: string }[]> = {
  access: [
    { id: "who", title: "Who can reach it" },
    { id: "guest-ssh", title: "Guest SSH access" },
    { id: "remote-typing", title: "Remote typing" },
    { id: "remote-sessions", title: "Remote sessions" },
  ],
  agents: [{ id: "rules", title: "Permission rules" }],
  advanced: [
    { id: "reports", title: "Usage and crash reports" },
    { id: "developer", title: "Developer settings" },
    { id: "nvim", title: "Remove arugula.nvim" },
  ],
};

/** The note over a section, what of it works anyway (`children`, with
 * `also` said after the note), and its machine's parts, dimmed. */
function Unreached({ id, why, also, children }: { id: Id; why: Away; also?: string; children?: ComponentChildren }) {
  return (
    <>
      <div class={why.wait ? "dim" : "settings-away-note"} role="status" data-machine-away>
        {why.line}
        {also && ` ${also}`}
      </div>
      {children}
      <div class="settings-away" inert data-machine-away-parts>
        {PARTS[id].map((p) => (
          <Section key={p.id} id={p.id} title={p.title} />
        ))}
      </div>
    </>
  );
}

/** One of the sections, with what it holds. */
function Sec({ id, summary, children }: { id: Id; summary?: string; children: ComponentChildren }) {
  const s = MACHINE_SECTIONS[id];
  return (
    <Section id={id} title={s.title} summary={summary} lede={s.lede}>
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
      ? { summary: "Reaching it…", line: "Reaching it…", wait: true }
      : client.state.roles
        ? { summary: "Its owner's", line: "Only its owner sees and changes these." }
        : undefined);
  const c = client!;
  return (
    <>
      <Band title="Who and what">
        {why ? (
          <Sec id="access" summary={why.summary}>
            <Unreached id="access" why={why} />
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
          {why ? (
            <Unreached id="agents" why={why} also={why.wait ? undefined : "Its agents' grants are Arugula cloud's, so they work as usual."}>
              {grants}
            </Unreached>
          ) : (
            <>
              {grants}
              <RulesSection client={c} />
            </>
          )}
        </Sec>
      </Band>
      <Band title="About it">
        {children}
        {why ? (
          <Sec id="advanced" summary={why.summary}>
            <Unreached id="advanced" why={why} />
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
