// Teams: making one, and for each its members and roles, its machines
// (an owner takes a member's out, #332), invite links (#134, #762), locking
// it, and the Labs flags control sets (#347). Roster admin stays in the
// shell: a roster change is signed with this device's key.

import { useEffect, useState } from "preact/hooks";
import { FLAGS } from "../../shared/proto.gen";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { Modal } from "../../shared/ui/modal";
import type { ControlSession, LabsFlags, PresignedInvite, Team } from "../control";
import type { TeamRole } from "../e2e/team.ts";
import { machineState } from "../ui/hosts";
import { ROLE_HELP, roleAs, roleLabel } from "../ui/roles";
import { TeamJoin } from "./machines";

/** The flags control may set: guest ssh and the agent catalog are each
 * machine's alone (`local_only`), set in its own Developer settings. */
const PUSHED = FLAGS.filter((f) => !f.local_only);

/** Labs flags set on control (#347), for an account's machines or a team's:
 * each unset (a machine follows its default), on or off. A machine's own
 * Developer settings win over these, and the account's over the team's. */
function LabsFlagList({ scope, what, flags, edit, set }: { scope: string; what: string; flags: Record<string, boolean>; edit: boolean; set: (name: string, on: boolean | null) => void }) {
  const named = PUSHED.filter((f) => flags[f.name] !== undefined).length;
  return (
    <details class="labs-flags" data-labs-flags={scope}>
      <summary>
        Labs flags <span class="dim">· {named ? `${named} set` : "none set"}</span>
      </summary>
      <p class="dim">{what}</p>
      <ul class="control-devices">
        {PUSHED.map((f) => {
          const v = flags[f.name];
          const value = v === undefined ? "" : v ? "on" : "off";
          return (
            <li key={f.name} data-labs-flag={f.name}>
              <span>
                {f.title}
                <span class="control-device-label dim">{f.about}</span>
              </span>
              {edit ? (
                <select
                  class="field"
                  value={value}
                  aria-label={`${f.title} for ${scope}`}
                  onChange={(e) => {
                    const x = (e.target as HTMLSelectElement).value;
                    set(f.name, x === "" ? null : x === "on");
                  }}
                >
                  <option value="">Not set</option>
                  <option value="on">On</option>
                  <option value="off">Off</option>
                </select>
              ) : (
                <span class="dim">{v === undefined ? "Not set" : v ? "On" : "Off"}</span>
              )}
            </li>
          );
        })}
      </ul>
    </details>
  );
}

export function Teams({ s, close }: { s: ControlSession; close: () => void }) {
  const [name, setName] = useState("");
  const [err, setErr] = useState("");
  const act = (f: () => Promise<unknown>) => f().catch((e: Error) => setErr(e.message));
  // Labs flags (#347): the account's and each team's; null until loaded,
  // left out by a control from before them, and for someone with no
  // machine that has Developer settings (strangers never see Labs).
  const [labs, setLabs] = useState<LabsFlags | null>(null);
  useEffect(() => {
    let live = true;
    s.labsFlags().then(
      (l) => live && setLabs(l.dev ? l : null),
      () => live && setLabs(null),
    );
    return () => {
      live = false;
    };
  }, [s]);
  const setFlag = (team: string | undefined) => (flag: string, on: boolean | null) =>
    act(async () => {
      const { flags } = await s.setLabsFlag(flag, on, team);
      setLabs((l) => l && (team ? { ...l, teams: { ...l.teams, [team]: { ...l.teams[team], flags } } } : { ...l, account: flags }));
    });
  return (
    <Modal close={close}>
      <h2>Teams</h2>
      <p class="dim" data-teams-intro>
        A team shares its machines with its members. Each member:
      </p>
      <ul class="control-roles">
        {ROLE_HELP.map(([r, what]) => (
          <li key={r}>
            <b>{roleLabel(r)}</b>: {what}
          </li>
        ))}
      </ul>
      <p class="dim">
        An invite link lets one person in right away (with Ask me first, an owner says yes to each). Any member can add their own machines to a team; its owners can take them out.{" "}
        <a href="https://docs.arugula.io/teams/" target="_blank" rel="noreferrer">
          More about teams
        </a>
      </p>
      {s.teams.map((t) => (
        <TeamSection key={t.team} s={s} t={t} act={act} labs={labs?.teams[t.team]} setFlag={setFlag(t.team)} />
      ))}
      {labs ? (
        <section class="team" data-account-labs>
          <h3>Your machines</h3>
          <LabsFlagList
            scope="your machines"
            what="For every machine of yours. A machine's own Developer settings win over these, and these over a team's."
            flags={labs.account}
            edit
            set={setFlag(undefined)}
          />
        </section>
      ) : null}
      <form
        class="control-code control-name"
        onSubmit={(e) => {
          e.preventDefault();
          void act(() => s.createTeam(name)).then(() => setName(""));
        }}
      >
        <input placeholder="New team's name" value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} aria-label="Team name" />
        <button type="submit" class="primary" disabled={!name.trim()}>
          Make a team
        </button>
      </form>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={close}>Done</button>
      </div>
    </Modal>
  );
}

function TeamSection({
  s,
  t,
  act,
  labs,
  setFlag,
}: {
  s: ControlSession;
  t: Team;
  act: (f: () => Promise<unknown>) => void;
  labs?: { flags: Record<string, boolean>; owner: boolean };
  setFlag: (name: string, on: boolean | null) => void;
}) {
  const [role, setRole] = useState<TeamRole>("editor");
  const [link, setLink] = useState<string | null>(null);
  // An invite that waits for an owner's yes (the old way), not presigned.
  const [askFirst, setAskFirst] = useState(false);
  const [linkAsks, setLinkAsks] = useState(false);
  const [linkWhy, setLinkWhy] = useState("");
  // Which button waits for a second click: "lock".
  const [confirming, setConfirming] = useState<string | null>(null);
  // The member whose Remove was clicked: a dialog asks.
  const [removing, setRemoving] = useState<string | null>(null);
  const owner = t.role === "owner";
  // A member's machine an owner is taking out of the team (#332).
  const [takingOut, setTakingOut] = useState<string | null>(null);
  const machines = s.daemons.filter((d) => d.team === t.team);
  // One-click links not used yet (#134), each with Cancel.
  const [unused, setUnused] = useState<PresignedInvite[]>([]);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    if (!owner) return setUnused([]);
    let live = true;
    s.presignedInvites(t.team).then(
      (l) => live && setUnused(l),
      () => live && setUnused([]),
    );
    return () => {
      live = false;
    };
  }, [s, t.team, owner, t.locked, t.roster.version, link, reload]);
  return (
    <section class="team" data-team={t.team}>
      <h3>
        {t.roster.name} {t.locked ? <span class="control-error">· locked</span> : null}
      </h3>
      {t.locked && !owner ? (
        <p class="dim" data-team-add-locked>
          It's locked: only its owners add machines to it until it's unlocked.
        </p>
      ) : (
        <>
          <p class="dim">
            Team id <CopyText inline text={t.team} data-team-id />.
          </p>
          <TeamJoin s={s} t={t} here />
        </>
      )}
      <h4 class="control-subgroup" data-team-members-head>
        Members <span class="dim">· {t.roster.members.length}</span>
      </h4>
      <ul class="control-devices control-rows" data-team-members={t.team}>
        {t.roster.members.map((m) => (
          <li key={m.account} data-member={m.account}>
            <span data-member-name>
              {t.names?.[m.account] ?? m.name}
              {m.account === s.account ? " (you)" : ""}
            </span>
            {owner && m.account !== s.account ? (
              <select
                class="field"
                value={m.role}
                aria-label={`${t.names?.[m.account] ?? m.name}'s role`}
                onChange={(e) => {
                  const r = (e.target as HTMLSelectElement).value as TeamRole;
                  act(() => s.changeTeam(t.team, (ms) => ms.map((x) => (x.account === m.account ? { ...x, role: r } : x))));
                }}
              >
                <RoleOptions />
              </select>
            ) : (
              <span class="dim">{roleLabel(m.role)}</span>
            )}
            {owner && m.account !== s.account ? (
              <button class="secondary danger small" data-remove-member={m.account} title="They lose access to the team's machines at once" onClick={() => setRemoving(m.account)}>
                Remove
              </button>
            ) : (
              <span />
            )}
          </li>
        ))}
      </ul>
      {removing ? (
        <ConfirmRemove
          title={`Remove ${t.names?.[removing] ?? t.roster.members.find((m) => m.account === removing)?.name ?? "them"} from ${t.roster.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            setRemoving(null);
            act(() => s.changeTeam(t.team, (ms) => ms.filter((x) => x.account !== removing)));
          }}
        >
          <p>They lose the team's machines at once. To come back, they need a new invite.</p>
        </ConfirmRemove>
      ) : null}
      <h4 class="control-subgroup" data-team-machines-head>
        Machines <span class="dim">· {machines.length}</span>
      </h4>
      {machines.length ? (
        <ul class="control-devices control-rows" data-team-machines={t.team}>
          {machines.map((d) => {
            const mine = !d.account || d.account === s.account;
            return (
              <li key={d.id} data-team-machine={d.id}>
                <span>
                  {d.name}
                  <span class="dim">{mine ? " · yours" : ` · ${d.owner_name || "a member"}'s`}</span>
                </span>
                <span class="dim" data-status>
                  {machineState(d.name)}
                </span>
                {owner && !mine ? (
                  <button class="secondary danger small" data-take-out={d.id} title="Take it out of the team; it stays its owner's" onClick={() => setTakingOut(d.id)}>
                    Take out
                  </button>
                ) : (
                  <span />
                )}
                {takingOut === d.id ? (
                  <p class="dim control-explain" data-take-out-explain>
                    {t.roster.name}'s members lose {d.name} at once. It stays {d.owner_name || "its owner"}'s, and they can add it again.{" "}
                    <button class="secondary small" data-take-out-cancel onClick={() => setTakingOut(null)}>
                      Cancel
                    </button>{" "}
                    <button
                      class="primary danger small"
                      data-take-out-go
                      onClick={() => {
                        setTakingOut(null);
                        act(() => s.moveDaemon(d.id, null));
                      }}
                    >
                      Take it out
                    </button>
                  </p>
                ) : null}
              </li>
            );
          })}
        </ul>
      ) : (
        <p class="dim">None yet.</p>
      )}
      {labs ? (
        <LabsFlagList
          scope={t.roster.name}
          what={
            labs.owner
              ? "For the team's machines. A machine's own Developer settings win over these, and so do its owner's flags for their machines."
              : "For the team's machines, set by its owners. A machine's own Developer settings win over these."
          }
          flags={labs.flags}
          edit={labs.owner}
          set={setFlag}
        />
      ) : null}
      {owner ? (
        <>
          <div class="control-code control-invite">
            <label>
              Invite{" "}
              <select class="field" value={role} aria-label="Invite as" data-invite-role={t.team} onChange={(e) => setRole((e.target as HTMLSelectElement).value as TeamRole)}>
                {ROLE_HELP.map(([r]) => (
                  <option key={r} value={r}>
                    {roleAs(r).replace(/^as /, "")}
                  </option>
                ))}
              </select>
            </label>
            <label class="control-check" title="Each person waits for an owner's yes, as before">
              <input type="checkbox" data-invite-ask-first checked={askFirst || role === "owner"} disabled={role === "owner"} onChange={(e) => setAskFirst((e.target as HTMLInputElement).checked)} />{" "}
              Ask me first
            </label>
            <button
              class="primary"
              data-invite={t.team}
              disabled={t.locked}
              onClick={() =>
                act(async () => {
                  const l = await s.makeInvite(t.team, role, askFirst);
                  setLinkAsks(l.asks);
                  setLinkWhy(l.why ?? "");
                  setLink(l.link);
                })
              }
            >
              Make a link
            </button>
          </div>
          {link && linkWhy ? (
            <p class="dim" data-invite-why>
              {linkWhy}, so this link asks you first. Rerun the install command on them to update.
            </p>
          ) : null}
          {link ? (
            <p>
              {linkAsks
                ? "Anyone with this link can ask to join (for a week); you approve each:"
                : "One person can join with this link, within a day, and you're told when they do. Send it only to them:"}
              <CopyText text={link} share data-invite-link />
            </p>
          ) : null}
          {unused.length ? (
            <>
              <p class="dim">One-click links nobody has used yet:</p>
              <ul class="control-devices control-rows" data-presigned-list>
                {unused.map((i) => (
                  <li key={i.key} data-presigned={i.key}>
                    <span>
                      {roleAs(i.role).replace(/^as /, "for ")}
                      {i.by === s.account ? "" : `, from ${i.by_name}`}
                    </span>
                    <span class="dim">{expiresIn(i.expires)}</span>
                    <button
                      class="secondary danger small"
                      data-cancel-presigned={i.key}
                      title="Nobody can join with this link any more"
                      onClick={() =>
                        act(async () => {
                          await s.cancelPresigned(t.team, i.key);
                          setReload((n) => n + 1);
                        })
                      }
                    >
                      Cancel
                    </button>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
          <div class="team-lock">
            <p class={confirming === "lock" ? "control-error" : "dim"} data-lock-warning={confirming === "lock" ? "" : undefined}>
              {t.locked
                ? "Locked: only owners reach the team's machines and add to it."
                : "Locking lets only owners reach the team's machines; open invites and requests are dropped."}
            </p>
            {confirming === "lock" ? <button class="secondary small" onClick={() => setConfirming(null)}>Cancel</button> : null}
            <button
              class={t.locked ? "secondary small" : confirming === "lock" ? "primary danger small" : "secondary danger small"}
              data-lock={t.team}
              onClick={() => {
                if (!t.locked && confirming !== "lock") return setConfirming("lock");
                setConfirming(null);
                setLink(null);
                act(() => s.lockTeam(t.team, !t.locked));
              }}
            >
              {t.locked ? "Unlock" : confirming === "lock" ? "Lock it" : "Lock team…"}
            </button>
          </div>
        </>
      ) : null}
    </section>
  );
}

/** How long a link has left, roughly. */
function expiresIn(at: number): string {
  const min = Math.max(1, Math.round((at - Date.now()) / 60_000));
  return min < 90 ? `expires in ${min} min` : `expires in ${Math.round(min / 60)} h`;
}

function RoleOptions() {
  return (
    <>
      {ROLE_HELP.map(([r]) => (
        <option key={r} value={r}>
          {roleLabel(r)}
        </option>
      ))}
    </>
  );
}
