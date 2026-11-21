// Settings' Machines scope (Arugula #1104, #1107): one of your machines,
// by the picker, in four sections (the Settings Doc's Machines tab):
// Access and sharing, Agents, Name and Advanced. Most of what's in them is
// the machine's own (who reaches it, its permission rules, reports,
// developer settings), which the page draws where it reaches machines
// (`PlatformApi.MachineSettings`, the app's). Then what Arugula cloud
// keeps about it: what its agents may do (../settings/agents), a part of
// Agents, and its name. A page that doesn't reach machines draws those
// two, and says the rest aren't here. The machines are those the platform's grants name
// (`/api/grants`, which lists every machine of yours).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { usePlatformApi } from "../../shared/platform-api";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { go, pathOf } from "../router";
import { MachineAgentGrants } from "../settings/agents";
import { machinesOf, type Grant } from "../settings/grants";

type Machine = { id: string; name: string };

let known: Promise<Machine[]> | null = null;
const listeners = new Set<() => void>();

/** Read your machines again (one was renamed). */
function readAgain() {
  known = null;
  listeners.forEach((fn) => fn());
}

/** Your machines, read once a page load (a failed read is tried again). */
export function useMachines(): { machines: Machine[] | null; err: string } {
  const [machines, setMachines] = useState<Machine[] | null>(null);
  const [err, setErr] = useState("");
  const [again, setAgain] = useState(0);
  useEffect(() => {
    const fn = () => setAgain((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
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
  }, [again]);
  return { machines, err };
}

export function MachineSettings({ machine, section }: { machine?: string; section?: string }) {
  const { machines, err } = useMachines();
  const api = usePlatformApi();
  // No machine named: the first one, at the section asked for.
  const first = machines?.[0]?.id;
  useEffect(() => {
    if (!machine && first) go(pathOf({ page: "settings", scope: "machine", machine: first, section }, names), true);
  }, [machine, first]);
  if (err) {
    return (
      <Section id="machine" title="Machines" summary="Couldn't load">
        <p class="control-error">{err}</p>
      </Section>
    );
  }
  if (!machines) return null;
  const m = machines.find((x) => x.id === machine);
  if (!m) {
    return (
      <Section id="machine" title="Machines" summary={machines.length ? "Not one of yours" : "No machines yet"} hooks={{ "data-machine-settings": "" }}>
        <p class="dim">{machines.length ? "That machine isn't one of yours (or it's gone). Pick one above." : "No machines on your account yet."}</p>
      </Section>
    );
  }
  const Own = api.MachineSettings;
  const grants = <MachineAgentGrants machine={m} />;
  const name = <MachineName key={m.id} m={m} />;
  return Own ? (
    <Own key={m.id} machine={m.id} name={m.name} grants={grants}>
      {name}
    </Own>
  ) : (
    <>
      <NotHere m={m} />
      <>
        <Section id="agents" title="Agents" lede="What its agents may do on Arugula cloud.">
          {grants}
        </Section>
      </>
      {name}
    </>
  );
}

/** Where the page reaches no machines: that the machine's own settings
 * aren't here, and why. No way off to its page (Arugula #1314). */
function NotHere({ m }: { m: Machine }) {
  return (
    <Section id="machine" title="Its own settings" summary="Not from this page" hooks={{ "data-machine-settings": m.id }}>
      <p class="settings-away-note" role="status">
        This page doesn&apos;t reach machines, so {m.name}&apos;s own settings (who can reach it, permission rules, reports, guest SSH and
        developer settings) aren&apos;t here. Settings in the app has them.
      </p>
    </Section>
  );
}

/** Its name on Arugula cloud (Arugula #500): the one the machines list,
 * the picker, chat and `@machine` call it. The old one keeps working in
 * mentions. */
function MachineName({ m }: { m: Machine }) {
  const [name, setName] = useState(m.name);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [done, setDone] = useState("");
  useEffect(() => setName(m.name), [m.name]);
  const rename = async (to: string) => {
    setBusy(true);
    setErr("");
    setDone("");
    try {
      const r = await platformFetch(`/api/daemons/${encodeURIComponent(m.id)}/name`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: to }),
      });
      const v = (await r.json().catch(() => ({}))) as { name?: string; error?: string };
      if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
      setDone(`Renamed to ${v.name ?? to}. Its old name still reaches it in mentions.`);
      readAgain();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section id="name" title="Name" summary={m.name} lede="What the machines list, chat and @mentions call it. Its old name keeps working in mentions." hooks={{ "data-machine-name": m.id }}>
      <form
        class="settings-add"
        aria-label="Rename"
        onSubmit={(e) => {
          e.preventDefault();
          const n = name.trim();
          if (n && n !== m.name) void rename(n);
        }}
      >
        <input class="field" aria-label="Name" value={name} maxLength={40} data-machine-rename-name onInput={(e) => setName(e.currentTarget.value)} />
        <button class="primary" type="submit" disabled={busy || !name.trim() || name.trim() === m.name} data-machine-rename>
          Rename
        </button>
      </form>
      {done && <p class="dim">{done}</p>}
      {err && <p class="control-error">{err}</p>}
    </Section>
  );
}
