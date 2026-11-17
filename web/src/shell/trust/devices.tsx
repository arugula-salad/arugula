// Machines (M17; the device list went with aru-141, #1004): the account's
// machines, each with when it was added and who approved it (#759) where
// this browser holds a device key, its team (*Move to…*, #100), and
// removing one; hosted machines and runs on cloud workspaces; and the
// words someone verifying you reads.

import { PasskeyResetWarning } from "./reset";
import { useEffect, useState } from "preact/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Modal } from "../../shared/ui/modal";
import { shownName, type ControlSession, type DirDaemon } from "../control";
import { fingerprint, type Cert } from "../e2e/cert.ts";
import { verifyCode, verifyWords } from "../e2e/words.ts";
import { directory } from "../hosts";
import { machinePlace, machineState, placeMeans } from "../ui/hosts";
import { AddedAt, openPanel, QrCode, since } from "./common";
import { HostedMachines } from "./hosted";
import { RunGrants } from "./run-grants";

/** #763: for someone sharing with you who wants to verify it's you: a QR
 * code for their camera, and the same as words for over the phone. From
 * the root this device pinned, not control's word for it. */
export function VerifyMe({ account, root }: { account: string; root: string }) {
  const [words, setWords] = useState<string[]>([]);
  useEffect(() => {
    void verifyWords(account, root).then(setWords);
  }, [account, root]);
  return (
    <details class="control-older" data-verify-me>
      <summary>Someone verifying you?</summary>
      <QrCode text={verifyCode(account, root)} label="QR code to verify your account" />
      <p class="dim">Or read them these words:</p>
      <p class="share-verify-words" data-verify-me-words>
        {words.join(" ")}
      </p>
    </details>
  );
}

/** *Move to…* on a machine (#100): into a team you're in (#332), or back
 * to just you. Only shown when there's somewhere to move it. */
function MoveMachine({ s, c, team, online }: { s: ControlSession; c: Cert; team: string | null; online: boolean }) {
  const [to, setTo] = useState<string | null | undefined>(undefined);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const teams = s.addableTeams();
  if (teams.length === 0 && !team) return null;
  const name = (id: string | null) => (id ? s.teams.find((t) => t.team === id)?.roster.name ?? "a team" : "just you");
  return (
    <div class="control-move" data-move={c.device}>
      <label class="dim control-move-label">
        Team{" "}
        <select
          class="field"
          data-move-to
          value={to === undefined ? (team ?? "") : (to ?? "")}
          onChange={(e) => {
            const v = (e.target as HTMLSelectElement).value || null;
            setErr("");
            setTo(v === team ? undefined : v);
          }}
        >
          <option value="">Just me</option>
          {teams.map((t) => (
            <option key={t.team} value={t.team}>
              {t.roster.name}
            </option>
          ))}
          {team && !teams.some((t) => t.team === team) ? <option value={team}>{name(team)}</option> : null}
        </select>
      </label>
      {to !== undefined ? (
        <p class="dim control-explain" data-move-explain>
          {to
            ? `Everyone in ${name(to)} sees ${c.name} and reaches it by their role${team ? `, and ${name(team)}'s members lose it` : ""}; its owners also see private panes. It stays yours, and you can take it out again.`
            : `${name(team)}'s members lose ${c.name} at once; only you reach it.`}
          {/* #778: a machine is in one team at a time; sharing reaches the
              other team without taking it from this one. */}
          {to && team ? ` To work with both, keep it in ${name(team)} and share a session with ${name(to)} instead (Share, beside the session's name).` : ""}
          {online ? " " : " It's offline, so it moves when it next connects. "}
          <button
            class="primary"
            data-move-go
            disabled={busy}
            onClick={() => {
              setBusy(true);
              s.moveDaemon(c.device, to).then(
                () => {
                  setBusy(false);
                  setTo(undefined);
                },
                (e: Error) => {
                  setBusy(false);
                  setErr(e.message);
                },
              );
            }}
          >
            Move
          </button>{" "}
          <button class="secondary" data-move-cancel onClick={() => setTo(undefined)}>
            Cancel
          </button>
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

/** What labels a machine in its row (#759): when it was added, where the
 * page knows (its certificate, checked by this browser's device key; a
 * machine reached with tokens has none), and *Details*. */
function DeviceLabel({ s, c }: { s: ControlSession; c: Cert }) {
  if (!c.created) return null;
  return (
    <span class="dim control-device-label">
      added <AddedAt at={c.created} /> <DeviceDetails s={s} c={c} />
    </span>
  );
}

export function DeviceDetails({ s, c }: { s: ControlSession; c: Cert }) {
  return (
    <details class="control-details" data-details={c.device}>
      <summary>Details</summary>
      <span class="control-details-body">
        <span>
          Fingerprint <span class="fingerprint-inline">{fingerprint(c.device)}</span>
        </span>
        {/* Who approved it, while the account still trusts it: a banner's
            device may be one it doesn't. */}
        {s.trusted.has(c.device) ? <span>{approvedText(s, c)}</span> : null}
      </span>
    </details>
  );
}

/** When a machine was added and which device approved it (#327), in its
 * row's *Details*. */
function approvedText(s: ControlSession, c: Cert): string {
  if (c.approver === c.device) return `Added ${since(c.created)}, the account's first device`;
  const by = s.trusted.get(c.approver);
  const on = !by ? "on a device since removed" : by.kind === "recovery" ? `with ${by.name}` : `on ${shownName(by.name)}${by.device === s.keys.id ? " (this browser)" : ""}`;
  return `Added ${since(c.created)}, approved ${on}`;
}

/** The account's machines (not someone else's shared with it, and not a
 * cloud workspace's box), as the directory lists them, by name. */
function ownMachines(s: ControlSession): DirDaemon[] {
  return s.daemons.filter((d) => !d.hosted && !d.account).sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

export function Devices({ s, close }: { s: ControlSession; close: () => void }) {
  const [err, setErr] = useState("");
  // The machine whose Remove was clicked: a dialog asks.
  const [removing, setRemoving] = useState<Cert | null>(null);
  const machines = ownMachines(s);
  return (
    <Modal close={close}>
      <h2>Machines</h2>
      {s.enrollment ? <PasskeyResetWarning s={s} /> : null}
      <p class="dim">
        Signed in as <b data-account={s.account}>{s.login}</b>. Your machines run your terminals.
      </p>
      {s.rootMismatch ? (
        <p class="control-error">Arugula cloud reports a different first device for this account than this browser pinned. New machines won't be trusted here.</p>
      ) : null}
      {machines.length === 0 ? <p class="dim">None yet.</p> : null}
      <ul class="control-devices" data-machines>
        {machines.map((d) => {
          const c = s.trusted.get(d.id) ?? d.cert;
          const team = d.team ? s.teams.find((t) => t.team === d.team)?.roster.name ?? "a team" : null;
          const path = d.name === directory.current && directory.path ? directory.path : d.urls.length ? "direct" : "relayed";
          return (
            <li key={d.id} data-device={d.id}>
              <span>
                {d.name}
                <span class="dim" data-status>
                  {" · "}
                  {machineState(d.name)}
                  {d.online ? ` · ${path}` : ""}
                </span>
                <DeviceLabel s={s} c={c} />
              </span>
              <button class="secondary danger small" data-remove={d.id} onClick={() => setRemoving({ ...c, name: d.name })}>
                Remove
              </button>
              {/* #778: its place and what that means, before a join or a
                  share elsewhere runs into it. */}
              <p class="dim control-explain" data-place={team ?? ""}>
                <b>{machinePlace(team)}</b>: {placeMeans(team)}
              </p>
              <MoveMachine s={s} c={{ ...c, name: d.name }} team={d.team ?? null} online={d.online} />
            </li>
          );
        })}
      </ul>
      <button class="secondary" data-add-machine onClick={openPanel("add")}>
        Add a machine…
      </button>
      <HostedMachines s={s} />
      <RunGrants s={s} />
      {s.enrollment && !s.rootMismatch ? <VerifyMe account={s.account} root={s.enrollment.root} /> : null}
      <p class="dim">
        Your name, passkeys and where you're signed in are in{" "}
        <button class="link" data-to-account onClick={openPanel("account")}>
          Sign-in and account…
        </button>
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      {removing ? (
        <ConfirmRemove
          title={`Remove ${removing.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            setRemoving(null);
            s.removeMachine(removing.device).catch((e: Error) => setErr(e.message));
          }}
        >
          <p data-remove-explain>
            It loses access at once: it's taken off your account. Arugula keeps running on it, reachable only locally. To add it back, join it again with{" "}
            <code>arugulad join</code>, which makes a new key.
          </p>
          <div data-remove-which>
            <b>{shownName(removing.name)}</b> <DeviceLabel s={s} c={removing} />
          </div>
        </ConfirmRemove>
      ) : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={close}>Done</button>
      </div>
    </Modal>
  );
}
