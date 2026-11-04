// Access (Arugula #681): who can reach this machine, and as what, in one
// dialog: *Access…* in the machine's settings (the machine menu, or the
// account menu's *This machine*), the owner's. The rows are the daemon's
// own answer (`access.get`, `GET /api/access`): its owner, the team's
// people by their role from the roster the machine checked itself, each
// session's shares with their floors, links and when they end, ssh guests
// and trust live now. Each row says whose word it is: this machine's, or
// the team's roster. Below them, this machine's agents' powers, which are
// Arugula cloud's word (`GET /api/grants`, as Agents' grants… reads it),
// labelled so, with what to do when cloud can't be asked. An older daemon
// has no `access.get` (a 404): the dialog says so, and where the shares
// still are.

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import { Time } from "../../shared/ui/time";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { AccessRow, AccessView } from "../../shared/proto.gen";
import { roleLabel } from "./roles";
import { directory } from "../hosts";
import { platformFetch } from "../../shared/maintenance";
import { POWERS, grantsOf, over, type Grant } from "./agent-grants";

let shown: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openAccess(client: Client) {
  shown = client;
  changed();
}

export function AccessLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!shown) return null;
  return (
    <AccessDialog
      client={shown}
      close={() => {
        shown = null;
        changed();
      }}
    />
  );
}

/** What a 404 on the operation means: the daemon predates it. */
export const OLDER_DAEMON =
  "This machine's daemon is older than this page, so it can't say who reaches it in one place. Update it (arugulad update); until then each session's share dialog and `arugula access` list its shares.";

/** What kind of machine this is, in a sentence. */
export function kindLine(v: AccessView): string {
  switch (v.kind) {
    case "local":
      return "Not joined to Arugula cloud: only its owner, and whoever they share a session with, can reach it.";
    case "team":
      return v.team
        ? `A machine of the team ${v.team.name}${v.team.locked ? " (locked)" : ""}: its people reach it by their team role.`
        : "A team machine whose roster this machine hasn't checked yet: nobody gets in through the team until it has.";
    case "hosted": {
      const whose = v.team ? `the team ${v.team.name}'s project` : "a personal project";
      if (v.purpose === "deploy" || v.purpose === "both") return `A deploy's box of ${whose}: nobody gets in, not even the project's owners.`;
      if (v.purpose === "snapshot") return `A box building a snapshot of ${whose}: its people are admitted here, and Arugula cloud routes nobody to it.`;
      if (v.team?.locked) return `A Cloud workspace of ${whose}, locked: only the team's owners get in.`;
      return v.team ? `A Cloud workspace of ${whose}: its people reach it by their team role.` : `A Cloud workspace of ${whose}: its account alone.`;
    }
    default:
      return "Joined to Arugula cloud as a personal machine: its account's devices, and whoever it shares a session with.";
  }
}

/** Where a row reaches, in words. */
export function whereLine(r: AccessRow, v: AccessView): string {
  const session = r.session != null ? v.sessions.find((s) => s.id === r.session) : undefined;
  const name = session ? `$${session.id} ${session.name}`.trim() : r.session != null ? `$${r.session}` : "";
  if (r.pane != null) return name ? `%${r.pane} in ${name}` : `%${r.pane}`;
  return name || "every session";
}

const WORDS: Record<string, string> = { this_machine: "this machine", roster: "the team's roster", control: "Arugula cloud" };

function Row({ r, v }: { r: AccessRow; v: AccessView }) {
  return (
    <li data-access-row={r.kind} data-access-who={r.who}>
      <div class="access-row-head">
        <b>{r.who}</b>
        <span data-access-role>{roleLabel(r.role)}</span>
        <span class="hint" data-access-where>
          {whereLine(r, v)}
        </span>
        <span class="access-word" data-access-word={r.word} title="Whose word this row is">
          {WORDS[r.word] ?? r.word}
        </span>
      </div>
      {(r.note || r.from_ms || r.until_ms) && (
        <span class="hint rule-where">
          {r.note}
          {r.from_ms ? (
            <>
              {r.note ? "; " : ""}from <Time at={r.from_ms} />
            </>
          ) : null}
          {r.until_ms ? (
            <>
              {r.note || r.from_ms ? "; " : ""}until <Time at={r.until_ms} />
            </>
          ) : null}
        </span>
      )}
    </li>
  );
}

/** This machine's live powers from `GET /api/grants`: by the device id the
 * daemon named, else by its name in control's directory. */
function Powers({ v }: { v: AccessView }) {
  const name = v.name || directory.current;
  const id = v.machine ?? directory.find(name)?.id;
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const joined = v.kind !== "local" && (directory.control || directory.joined);
  useEffect(() => {
    if (!joined) return;
    let live = true;
    platformFetch("/api/grants", { credentials: "same-origin" })
      .then(async (r) => {
        const got = (await r.json().catch(() => null)) as { grants?: Grant[]; error?: string } | null;
        if (!r.ok || !got?.grants) throw new Error(got?.error ?? `${r.status}`);
        if (live) setGrants(grantsOf(got.grants, { id, name }));
      })
      .catch(() => live && setError("Arugula cloud didn't answer, so its agents' powers aren't shown: `arugula grants ls` lists them."));
    return () => void (live = false);
  }, [joined, id, name]);
  if (v.kind === "local") return <p class="hint" data-access-powers-none>Not joined to Arugula cloud: its agents have no powers there.</p>;
  if (!joined) return <p class="hint" data-access-powers-error>Arugula cloud has them: open this machine there to see them here, or `arugula grants ls`.</p>;
  if (error) return <p class="hint" data-access-powers-error>{error}</p>;
  if (grants === null) return <p class="hint">Asking Arugula cloud…</p>;
  if (!grants.length) return <p class="hint" data-access-powers-none>None: its agents can't do anything on Arugula cloud.</p>;
  return (
    <ul class="rules-list access-rows" data-access-powers>
      {grants.map((g) => (
        <li key={g.id} data-access-power={g.power}>
          <div class="access-row-head">
            <b>{POWERS[g.power] ?? g.power}</b>
            <span class="hint">over {over(g)}</span>
            <span class="access-word" data-access-word="control" title="Whose word this row is">
              {WORDS.control}
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}

function AccessDialog({ client, close }: { client: Client; close: () => void }) {
  const [view, setView] = useState<AccessView | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const res = await client.request(OPS["access.get"].method, opPath("access.get"));
        if (res.status === 404) throw new Error(OLDER_DAEMON);
        const v = await res.json<AccessView | { error?: string }>().catch(() => null);
        if (v === null) throw new Error(`This machine's daemon didn't answer (${res.status}): reopen this once it's connected.`);
        if (!res.ok) throw new Error((v as { error?: string }).error ?? `couldn't read who reaches it (${res.status})`);
        if (live) setView(v as AccessView);
      } catch (e) {
        if (live) setError((e as Error).message);
      }
    })();
    return () => {
      live = false;
    };
  }, [client]);
  const name = view?.name || directory.current;
  return (
    <Modal close={close} label="Access" class="rules access-view">
      <h2>Who can reach {name}</h2>
      <p class="hint">
        The people, links and guests that reach this machine, and as what, from what it holds. Each row says whose word it is: this
        machine&apos;s own files, the team&apos;s roster it checked itself, or Arugula cloud.
      </p>
      {error && (
        <p class="error" data-access-error>
          {error}
        </p>
      )}
      {!error && view === null && <p class="hint">Loading…</p>}
      {view && (
        <>
          <p class="hint" data-access-kind>
            {kindLine(view)}
          </p>
          {view.notes.map((n) => (
            <p class="hint" data-access-note key={n}>
              {n}
            </p>
          ))}
          {view.rows.length ? (
            <ul class="rules-list access-rows" data-access-rows>
              {view.rows.map((r, i) => (
                <Row key={`${r.kind}-${r.principal ?? r.who}-${r.session ?? ""}-${r.pane ?? ""}-${i}`} r={r} v={view} />
              ))}
            </ul>
          ) : (
            <p data-access-nobody>Nobody can reach it.</p>
          )}
          {view.guest_ssh === false && <p class="hint">Guest SSH access is off: no ssh guest can be invited until you turn it on.</p>}
          <h3>Its agents&apos; powers</h3>
          <p class="hint">
            What the agents on {name} may do on Arugula cloud, as you. That is Arugula cloud&apos;s word, not this machine&apos;s: change it in
            Account settings, under Agents, or with <code>arugula grants</code>.
          </p>
          <Powers v={view} />
        </>
      )}
      <div class="prompt-buttons">
        <button class="primary" type="button" onClick={close}>
          Done
        </button>
      </div>
    </Modal>
  );
}
