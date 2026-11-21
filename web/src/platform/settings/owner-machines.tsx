// Team and billing's Machines (Arugula #1106): the computers an owner has,
// as control's directory lists them (`/api/directory`, which the app's
// origin reaches): a team's are those shared with it, Personal's are yours
// that are no team's. Cloud workspaces aren't listed: each is a project's.
// One of yours opens its settings (This machine); someone else's says
// whose it is. Sharing one with a team, or taking it out, is done on
// control's page (a device signs it there; with no device key, control's
// word does, aru-141), so that row opens it (`#teams`).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { ago } from "../../shared/time";
import { ExternalIcon } from "../../shared/ui/icons";
import { Section } from "../../shared/ui/settings-section";
import { Chip } from "../../shared/ui/state-chip";
import { names } from "../links";
import { useMachines } from "../pages/machine-settings";
import { follow, pathOf } from "../router";
import { controlOrigin } from "../session";
import { joined, plural } from "./summary";

/** A machine as the directory lists it: what this section reads. */
export interface DirMachine {
  id: string;
  name: string;
  online: boolean;
  /** When control last heard from it, in ms. */
  last_seen: number | null;
  /** The team it's in, if any. */
  team?: string | null;
  /** Someone else's: its owner's account and name. */
  account?: string;
  owner_name?: string;
  /** A Cloud workspace's box. */
  hosted?: unknown;
}

let known: Promise<DirMachine[]> | null = null;

/** The directory, read once a page load (a failed read is tried again). */
function useDirectory(): { all: DirMachine[] | null; err: string } {
  const [all, setAll] = useState<DirMachine[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    known ??= platformFetch("/api/directory", { credentials: "same-origin" }).then(async (r) => {
      const v = (await r.json().catch(() => ({}))) as { daemons?: DirMachine[]; error?: string };
      if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
      return v.daemons ?? [];
    });
    known.then(
      (d) => live && setAll(d),
      (e: Error) => {
        known = null;
        if (live) setErr(e.message);
      },
    );
    return () => void (live = false);
  }, []);
  return { all, err };
}

/** An owner's computers, by name: a team's, or Personal's (yours, in no team). */
export function machinesOf(all: DirMachine[], owner: string): DirMachine[] {
  return all
    .filter((d) => !d.hosted && (owner === "personal" ? !d.account && !d.team : d.team === owner))
    .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/** What the list says of them: how many, and how many answer now. */
export function machinesSummary(list: DirMachine[], team: boolean): string {
  if (list.length === 0) return team ? "None shared yet" : "No machines yet";
  const on = list.filter((d) => d.online).length;
  return joined(plural(list.length, "machine"), on === list.length ? (on === 1 ? "online" : "all online") : `${on} online`);
}

export function OwnerMachines({ owner, name, teams }: { owner: string; name: string; teams: boolean }) {
  const { all, err } = useDirectory();
  // Those with settings in This machine (#1104's picker reads the same list).
  const { machines: mine } = useMachines();
  const team = owner !== "personal";
  const list = all ? machinesOf(all, owner) : null;
  return (
    <Section
      id="machines"
      title="Machines"
      summary={list ? machinesSummary(list, team) : err ? "Couldn't load" : undefined}
      count={list?.length}
      lede={team ? `The machines shared with ${name}. Its members can open them.` : "Your machines that aren't a team's."}
      hooks={{ "data-owner-machines": owner }}
    >
      {err && <p class="control-error">{err}</p>}
      {list && (
        <ul class="platform-list settings-rows">
          {list.map((d) => {
            const state = d.online ? "Online" : d.last_seen ? `Offline, seen ${ago(d.last_seen)}` : "Offline";
            if (d.account)
              return (
                <li key={d.id} data-owner-machine={d.id}>
                  <span>
                    <b>{d.name}</b> <span class="dim">{d.owner_name ? `${d.owner_name}'s` : ""}</span>
                  </span>
                  <Chip dot tone={d.online ? "ok" : "muted"}>
                    {state}
                  </Chip>
                </li>
              );
            // Yours: its settings, or its page where Settings doesn't list it yet.
            const href = mine?.some((m) => m.id === d.id)
              ? pathOf({ page: "settings", scope: "machine", machine: d.id }, names)
              : pathOf({ page: "find", what: "machines", at: { at: "machine", machine: d.id } }, names);
            return (
              <li key={d.id} data-owner-machine={d.id}>
                <a class="settings-open" href={href} data-owner-machine-open={d.id} onClick={(e) => follow(e, href)}>
                  <span class="settings-open-name">{d.name}</span>
                  <span class="dim">{state}</span>
                </a>
              </li>
            );
          })}
          {list.length === 0 && <li class="dim">{team ? `No machines are shared with ${name} yet.` : "None yet."}</li>}
        </ul>
      )}
      {(team || teams) && (
        <ul class="platform-list settings-rows">
          <ControlRow
            href={`${controlOrigin()}/#teams`}
            label={team ? `Share a machine with ${name}, or take one out` : "Move a machine into a team"}
            why="On your machines page."
            hook="machines"
          />
        </ul>
      )}
    </Section>
  );
}

/** A row a device signs (or the app can't reach yet): it opens control's
 * page, marked ↗, with why under it. */
export function ControlRow({ href, label, why, hook }: { href: string; label: string; why: string; hook: string }) {
  return (
    <li>
      <a class="settings-open" href={href} data-settings-control={hook}>
        <span class="settings-open-name">
          {label} <ExternalIcon size="xs" />
        </span>
        <span class="dim">{why}</span>
      </a>
    </li>
  );
}
