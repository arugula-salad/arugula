// Runs on hosted workspaces (M211), under Hosted machines in Devices and
// machines: for each project this account owns that has a delegation, its
// standing grant ("One standing grant per project starts runs"). Since DK6
// (#387, aru-126) each is confirmed with one of the account's passkeys over
// its bytes, not signed by this device: Allow runs makes a grant for a
// number of days; Renew makes a new one, which takes the old one's place
// (one passkey prompt); Revoke takes back every grant the project has under
// its delegation, and can stop the runs going too. The platform keeps them;
// the hosted machines check them against the account's passkey log.
//
// One project's is its own dialog, `#run-grant=prj_…` (Arugula #604): the
// link from the project's settings on the platform page, and the phone
// app's web sheet with `&app`, which goes back to the app when closed.

import { useEffect, useState } from "preact/hooks";
import { Time } from "../../shared/ui/time";
import { api, type ControlSession, type RunGrantView } from "../control";
import type { Delegation } from "../e2e/hosted.ts";
import { GRANT_DAYS } from "../e2e/run-grant.ts";
import { platformFetch } from "../../shared/maintenance";
import { Modal } from "../../shared/ui/modal";
import { backToApp, clearHash } from "./common";
import type { PlatformProjects } from "../../shared/proto.gen";

interface Row {
  id: string;
  name: string;
  /** The project's live delegation, from control. */
  delegation: Delegation;
  delegationId: string;
  view: RunGrantView | null;
}

const DAYS = [30, GRANT_DAYS, 365];

/** One project's grant, as its own dialog: `#run-grant=prj_…`, and `&app`
 * from the phone app's web sheet. */
export const RUN_GRANT_HASH = /^#run-grant=(prj_[0-9a-f]{32})(&app)?$/;

export function RunGrantPrompt({ s, project, app = false }: { s: ControlSession; project: string; app?: boolean }) {
  const close = app ? backToApp : clearHash;
  return (
    <Modal close={close}>
      <h2>Runs on cloud workspaces</h2>
      <div data-run-grant-prompt={project}>
        <RunGrants s={s} only={project} />
      </div>
      <div class="prompt-buttons">
        <button class="secondary" data-run-grant-back onClick={close}>
          {app ? "Back to the app" : "Close"}
        </button>
      </div>
    </Modal>
  );
}

/** Each project this account owns that has a delegation, or only the
 * project `only`. */
export function RunGrants({ s, only }: { s: ControlSession; only?: string }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [busy, setBusy] = useState("");
  const [stop, setStop] = useState<Record<string, boolean>>({});
  const [days, setDays] = useState<Record<string, number>>({});
  const [err, setErr] = useState("");
  const load = async () => {
    const [d, p] = await Promise.all([s.delegations(), api<PlatformProjects>("/api/projects", undefined, platformFetch).catch(() => null)]);
    const owned = (p?.projects ?? [])
      .filter((x) => !x.team || x.team.role === "owner")
      .filter((x) => !only || x.id === only)
      .map((x) => ({ id: x.id, name: x.name, owner: x.team ? `team:${x.team.id}` : `account:${s.account}` }));
    const out: Row[] = [];
    for (const x of owned) {
      const live = d.delegations.find((l) => l.delegation.project === x.id && l.delegation.owner === x.owner && l.delegation.key === d.key && !l.revoke && !l.ended);
      if (!live) continue;
      out.push({ id: x.id, name: x.name, delegation: live.delegation, delegationId: live.id, view: await s.runGrant(x.id).catch(() => null) });
    }
    setRows(out);
  };
  useEffect(() => void load().catch((e: Error) => setErr(e.message)), []);
  if (!rows?.length) {
    if (err) return <p class="control-error">{err}</p>;
    // Its own dialog says why there's nothing to allow.
    return only && rows ? (
      <p class="dim" data-run-grant-none>
        Nothing to allow here: you don't own this project, or it has no cloud workspaces to run on.
      </p>
    ) : null;
  }
  // A passkey of this account's confirms a grant or a revoke: an account
  // with none is told how to add one; a window with no WebAuthn (the Linux
  // app's) sends you to a browser.
  const webauthn = typeof PublicKeyCredential !== "undefined";
  const mine = webauthn && s.passkeys > 0;
  const act = (id: string, f: () => Promise<unknown>) => async () => {
    setBusy(id);
    setErr("");
    try {
      await f();
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy("");
  };
  const daysOf = (r: Row) => days[r.id] ?? GRANT_DAYS;
  const pick = (r: Row) => (
    <select aria-label="How long" data-run-grant-days={r.id} value={daysOf(r)} onChange={(e) => setDays({ ...days, [r.id]: Number((e.target as HTMLSelectElement).value) })}>
      {DAYS.map((n) => (
        <option key={n} value={n}>
          for {n} days
        </option>
      ))}
    </select>
  );
  return (
    <>
      <h3 class="control-group">Runs on cloud workspaces</h3>
      <ul class="control-devices" data-run-grants>
        {rows.map((r) => {
          const g = r.view?.grant;
          const renew = !!g && (r.view?.renew || r.view?.stale);
          return (
            <li key={r.id} data-project={r.id} data-run-grant={g ? (renew ? "renew" : "yes") : r.view?.offer ? "offered" : "no"}>
              <span>
                {r.name}
                <span class="dim">
                  {g
                    ? r.view?.stale
                      ? " · the grant is for an earlier delegation: sign it again"
                      : (
                        <>
                          {" "}
                          · runs may start {g.grant.expires ? (
                            <>
                              until it expires <Time at={g.grant.expires} />
                            </>
                          ) : (
                            "until it's revoked"
                          )}
                          {r.view?.renew ? ": renew it" : ""}
                        </>
                      )
                    : r.view?.offer
                      ? " · has cloud workspaces: allow runs on them"
                      : " · no runs"}
                </span>
              </span>
              {g ? (
                <span>
                  {renew ? (
                    <>
                      {pick(r)}{" "}
                      <button
                        class="secondary small"
                        disabled={!!busy || !mine}
                        data-renew-run-grant={r.id}
                        onClick={act(r.id, () => s.signRunGrant(r.delegation, daysOf(r)))}
                      >
                        {r.view?.stale ? "Sign again" : "Renew"}
                      </button>{" "}
                    </>
                  ) : null}
                  <label class="control-check dim">
                    <input type="checkbox" data-stop-running checked={!!stop[r.id]} onChange={(e) => setStop({ ...stop, [r.id]: (e.target as HTMLInputElement).checked })} /> also
                    stop the runs and agent sessions going
                  </label>{" "}
                  <button class="secondary danger small" disabled={!!busy || !mine} data-revoke-run-grant={r.id} onClick={act(r.id, () => s.revokeRunGrants(r.id, g.grant.delegation, !!stop[r.id]))}>
                    Revoke
                  </button>
                </span>
              ) : (
                <span>
                  {pick(r)}{" "}
                  <button class="secondary small" disabled={!!busy || !mine} data-sign-run-grant={r.id} onClick={act(r.id, () => s.signRunGrant(r.delegation, daysOf(r)))}>
                    Allow runs
                  </button>
                </span>
              )}
            </li>
          );
        })}
      </ul>
      <p class="dim control-explain">
        <b>Allow runs</b> asks for one of your passkeys to confirm a standing grant: for as long as you pick, Arugula may start runs and
        instruct them on every cloud workspace of the project, running whatever command the project's runs start, with none of your devices
        awake. A thread's agent sessions run under it too: with no grant, <code>@arugula</code> starts nothing. Each workspace checks your
        passkey against your account's published passkeys. <b>Renew</b> confirms a new one in its place. Deploys still ask you every time.{" "}
        <b>Revoke</b> (a passkey too) stops new runs and ends agent sessions; a run or an agent's turn going finishes unless you stop them too. A
        workspace that's asleep when you revoke is told later; one that's never told keeps honouring the grant until it expires.
      </p>
      {!webauthn ? (
        <p class="dim" data-run-grant-elsewhere>
          This window can't use passkeys: allow, renew or revoke runs from a browser or a phone of yours.
        </p>
      ) : s.passkeys === 0 ? (
        <p class="dim" data-run-grant-no-passkey>
          Runs are allowed and revoked with a passkey, and your account has none. Add one first (the account menu, Sign-in and account), then
          allow runs here.
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
