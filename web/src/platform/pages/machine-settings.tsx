// Settings' This machine scope (Arugula #1104): one of your machines, by
// the picker. For now one section that opens the machine's own page, where
// its settings are (Access, Permission rules, usage and crash reports,
// Guest SSH, Developer settings, its name); the app draws them here in
// Arugula #1107. The machines are those the platform's grants name
// (`/api/grants`, as Agents on your machines reads them).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { machinesOf, type Grant } from "../settings/grants";

type Machine = { id: string; name: string };

let known: Promise<Machine[]> | null = null;

/** Your machines, read once a page load (a failed read is tried again). */
export function useMachines(): { machines: Machine[] | null; err: string } {
  const [machines, setMachines] = useState<Machine[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    known ??= platformFetch("/api/grants", { credentials: "same-origin" }).then(async (r) => {
      const v = (await r.json().catch(() => ({}))) as { grants?: Grant[]; error?: string };
      if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
      return machinesOf(v.grants ?? []);
    });
    known.then(
      (m) => live && setMachines(m),
      (e: Error) => {
        known = null;
        if (live) setErr(e.message);
      },
    );
    return () => void (live = false);
  }, []);
  return { machines, err };
}

export function MachineSettings({ machine }: { machine?: string }) {
  const { machines, err } = useMachines();
  // No machine named: the first one.
  const first = machines?.[0]?.id;
  useEffect(() => {
    if (!machine && first) go(pathOf({ page: "settings", scope: "machine", machine: first }, names), true);
  }, [machine, first]);
  if (err) {
    return (
      <Section id="machine" title="This machine" summary="Couldn't load">
        <p class="control-error">{err}</p>
      </Section>
    );
  }
  if (!machines) return null;
  const m = machines.find((x) => x.id === machine);
  if (!m) {
    return (
      <Section id="machine" title="This machine" summary={machines.length ? "Not one of yours" : "No machines yet"} hooks={{ "data-machine-settings": "" }}>
        <p class="dim">{machines.length ? "That machine isn't one of yours (or it's gone). Pick one above." : "No machines on your account yet."}</p>
      </Section>
    );
  }
  const page = pathOf({ page: "find", what: "machines", at: { at: "machine", machine: m.id } }, names);
  return (
    <Section
      id="machine"
      title="This machine"
      summary="On its own page for now"
      lede={`${m.name}'s settings are on its page, under This machine in its menu: who can reach it, permission rules, reports, guest SSH, developer settings and its name.`}
      hooks={{ "data-machine-settings": m.id }}
    >
      <ul class="platform-list settings-rows">
        <li>
          <a class="settings-open" href={page} data-machine-settings-open={m.id} onClick={(e) => follow(e, page)}>
            <span class="settings-open-name">Open {m.name}'s page</span>
          </a>
        </li>
      </ul>
    </Section>
  );
}
