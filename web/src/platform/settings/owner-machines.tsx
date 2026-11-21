// Team and billing's Machines (Arugula #1106): the computers an owner has,
// as control's directory lists them (`/api/directory`, which the app's
// origin reaches): a team's are those shared with it, Personal's are yours
// that are no team's. Cloud workspaces aren't listed: each is a project's.
// Each opens its settings (This machine), or its page where Settings
// doesn't list it yet. Changes are in place, on control's word (aru-141):
// removing one of yours, taking one out of a team (yours, or any of its
// as the team's owner), moving one of yours into a team, and Add a machine,
// the `arugulad join` command with Copy. Until the hub serves those routes
// on the app's host (`/api/teams` answers 404 here), moving and taking
// out stay a ↗ row to control's page (`#teams`), and removing is there too.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { movesInto } from "../../shared/team-adds";
import { ago } from "../../shared/time";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { ExternalIcon } from "../../shared/ui/icons";
import { Section } from "../../shared/ui/settings-section";
import { Chip } from "../../shared/ui/state-chip";
import { names } from "../links";
import { useMachines } from "../pages/machine-settings";
import { follow, pathOf } from "../router";
import { controlOrigin } from "../session";
import { joined, plural } from "./summary";
import { hubCall, useTeams, type HubTeam } from "./teams";

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
const listeners = new Set<() => void>();

/** Read the directory again, after a change, for every section showing it. */
function reloadDirectory() {
  known = null;
  listeners.forEach((fn) => fn());
}

/** The directory, read once a page load and again after a change (a failed
 * read is tried again). */
function useDirectory(): { all: DirMachine[] | null; err: string } {
  const [all, setAll] = useState<DirMachine[] | null>(null);
  const [err, setErr] = useState("");
  const [again, setAgain] = useState(0);
  useEffect(() => {
    const fn = () => setAgain((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  useEffect(() => {
    let live = true;
    known ??= platformFetch("/api/directory", { credentials: "same-origin" }).then(async (r) => {
      const v = (await r.json().catch(() => ({}))) as { daemons?: DirMachine[]; error?: string };
      if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
      return v.daemons ?? [];
    });
    known.then(
      (d) => {
        if (!live) return;
        setAll(d);
        setErr("");
      },
      (e: Error) => {
        known = null;
        if (live) setErr(e.message);
      },
    );
    return () => void (live = false);
  }, [again]);
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

export function OwnerMachines({ owner, name, role, teams }: { owner: string; name: string; role: string | undefined; teams: boolean }) {
  const { all, err } = useDirectory();
  // Those with settings in This machine (#1104's picker reads the same list).
  const { machines: mine } = useMachines();
  const hub = useTeams();
  const team = owner !== "personal";
  const list = all ? machinesOf(all, owner) : null;
  // The hub's teams, when it answers them here: then changes are in place.
  const known = hub && "teams" in hub ? hub.teams : null;
  const t = known?.find((x) => x.team === owner);
  // Teams a machine of mine may move into (control's `in_team`).
  const into = (known ?? []).filter((x) => x.team !== owner && movesInto(x));
  // May I add a machine to it: as control's rule, or by my role till the hub says.
  const adds = !team || (t ? movesInto(t) : role === "owner" || role === "editor");
  const [err2, setErr2] = useState("");
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<DirMachine | null>(null);
  const [takingOut, setTakingOut] = useState<DirMachine | null>(null);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr2("");
    try {
      await f();
      reloadDirectory();
    } catch (e) {
      setErr2((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  // A move on control's word: unsigned, which control takes from the
  // machine's owner, or a team's owner taking it out. A machine from before
  // control's moves says so (409).
  const move = (d: DirMachine, to: HubTeam | null) =>
    act(() => hubCall("POST", `/api/daemons/${encodeURIComponent(d.id)}/team`, { team: to?.pin ?? null, at: Date.now(), by: "", sig: "" }));
  const theirs = (d: DirMachine) => (d.owner_name ? `${d.owner_name}'s` : "its owner's");
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
                  {t?.role === "owner" && (
                    <span class="settings-row-act">
                      <button class="secondary danger small" disabled={busy} data-take-out={d.id} title={`Take it out of ${name}; it stays ${theirs(d)}`} onClick={() => setTakingOut(d)}>
                        Take out
                      </button>
                    </span>
                  )}
                </li>
              );
            // Yours: its settings, or its page where Settings doesn't list it yet.
            const href = mine?.some((m) => m.id === d.id)
              ? pathOf({ page: "settings", scope: "machine", machine: d.id }, names)
              : pathOf({ page: "find", what: "machines", at: { at: "machine", machine: d.id } }, names);
            if (!known)
              return (
                <li key={d.id} data-owner-machine={d.id}>
                  <a class="settings-open" href={href} data-owner-machine-open={d.id} onClick={(e) => follow(e, href)}>
                    <span class="settings-open-name">{d.name}</span>
                    <span class="dim">{state}</span>
                  </a>
                </li>
              );
            return (
              <li key={d.id} data-owner-machine={d.id}>
                <b>{d.name}</b>
                <span class="dim">{state}</span>
                <span class="settings-row-act">
                  <a class="secondary small" href={href} data-owner-machine-open={d.id} onClick={(e) => follow(e, href)}>
                    Settings
                  </a>
                  {team ? (
                    <button class="secondary small" disabled={busy} data-take-out={d.id} title={`Take it out of ${name}; it stays yours`} onClick={() => setTakingOut(d)}>
                      Take out of team
                    </button>
                  ) : (
                    into.length > 0 && (
                      <select
                        class="field"
                        value=""
                        disabled={busy}
                        aria-label={`Move ${d.name} into a team`}
                        data-move-into={d.id}
                        onChange={(e) => {
                          const to = into.find((x) => x.team === e.currentTarget.value);
                          if (to) void move(d, to);
                        }}
                      >
                        <option value="">Move into a team…</option>
                        {into.map((x) => (
                          <option key={x.team} value={x.team}>
                            {x.roster.name}
                          </option>
                        ))}
                      </select>
                    )
                  )}
                  <button class="secondary danger small" disabled={busy} data-remove-machine={d.id} onClick={() => setRemoving(d)}>
                    Remove
                  </button>
                </span>
              </li>
            );
          })}
          {list.length === 0 && <li class="dim">{team ? `No machines are shared with ${name} yet.` : "None yet."}</li>}
        </ul>
      )}
      {known && t && adds && all && <BringIn all={all} t={t} name={name} busy={busy} bring={(d) => void move(d, t)} />}
      {err2 && <p class="control-error">{err2}</p>}
      {!known && (team || teams) && (
        <ul class="platform-list settings-rows">
          <ControlRow
            href={`${controlOrigin()}/#teams`}
            label={team ? `Share a machine with ${name}, or take one out` : "Move a machine into a team"}
            why="On your machines page."
            hook="machines"
          />
        </ul>
      )}
      {!known && hub !== null && (
        <ul class="platform-list settings-rows">
          <ControlRow href={`${controlOrigin()}/#devices`} label="Remove a machine" why="On your machines page." hook="devices" />
        </ul>
      )}
      {adds && <AddMachine team={team ? owner : null} name={name} />}
      {removing && (
        <ConfirmRemove
          title={`Remove ${removing.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            const d = removing;
            setRemoving(null);
            void act(() => hubCall("DELETE", `/api/daemons/${encodeURIComponent(d.id)}`));
          }}
        >
          <p data-remove-explain>
            It loses access at once: it's taken off your account. Arugula keeps running on it, reachable only locally. To add it back, join it again with <code>arugulad join</code>, which makes a new key.
          </p>
        </ConfirmRemove>
      )}
      {takingOut && (
        <ConfirmRemove
          title={`Take ${takingOut.name} out of ${name}?`}
          label="Take it out"
          cancel={() => setTakingOut(null)}
          go={() => {
            const d = takingOut;
            setTakingOut(null);
            void move(d, null);
          }}
        >
          <p>
            {name}'s members lose {takingOut.name} at once. It stays {takingOut.account ? theirs(takingOut) : "yours"}, and {takingOut.account ? "they" : "you"} can add it again.
          </p>
        </ConfirmRemove>
      )}
    </Section>
  );
}

/** Bring one of yours into the team: a machine of yours that isn't in it,
 * moved on control's word. */
function BringIn({ all, t, name, busy, bring }: { all: DirMachine[]; t: HubTeam; name: string; busy: boolean; bring: (d: DirMachine) => void }) {
  const yours = all.filter((d) => !d.hosted && !d.account && d.team !== t.team).sort((a, b) => a.name.localeCompare(b.name));
  const [pick, setPick] = useState("");
  if (!yours.length) return null;
  return (
    <form
      class="settings-add"
      data-bring-in={t.team}
      onSubmit={(e) => {
        e.preventDefault();
        const d = yours.find((x) => x.id === pick);
        if (d) bring(d);
        setPick("");
      }}
    >
      <label class="settings-field">
        Bring in one of yours
        <select class="field" value={pick} data-bring-in-pick onChange={(e) => setPick(e.currentTarget.value)}>
          <option value="">Pick a machine…</option>
          {yours.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.team ? " (in another team)" : ""}
            </option>
          ))}
        </select>
      </label>
      <button class="primary" disabled={busy || !pick} data-bring-in-go>
        Share with {name}
      </button>
    </form>
  );
}

/** Add a machine: install Arugula on it, then the join command, which
 * prints a link to control's page that adds it (to the team, with
 * `--team`). */
function AddMachine({ team, name }: { team: string | null; name: string }) {
  return (
    <div data-add-machine>
      <h3>Add a machine</h3>
      <p class="dim">
        <a href="https://arugula.io/#install" target="_blank" rel="noopener">
          Install Arugula
        </a>{" "}
        on it, then run this there. It prints a link: open it to add the machine{team ? ` to ${name}` : ""}.
      </p>
      <CopyText text={`arugulad join ${controlOrigin()}${team ? ` --team ${team}` : ""}`} data-join-cmd />
    </div>
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
