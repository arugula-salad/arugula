// Runs on hosted workspaces (M211), under Hosted machines in Devices and
// machines: for each project this account owns that has a delegation, its
// standing grant ("One standing grant per project starts runs"). Allow
// runs signs it with this device (here or on the phone) for a number of
// days; Renew signs a revoke and a new one; Revoke signs a revoke of every
// grant the project has under its delegation, and can stop the runs going
// too. The platform keeps them; the hosted machines check them.

import { useEffect, useState } from "preact/hooks";
import { api, type ControlSession, type RunGrantView } from "../control";
import type { Delegation } from "../e2e/hosted.ts";
import { GRANT_DAYS } from "../e2e/run-grant.ts";
import { platformFetch } from "../../shared/maintenance";
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
const day = (ms: number) => new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

export function RunGrants({ s }: { s: ControlSession }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [busy, setBusy] = useState("");
  const [stop, setStop] = useState<Record<string, boolean>>({});
  const [days, setDays] = useState<Record<string, number>>({});
  const [err, setErr] = useState("");
  const load = async () => {
    const [d, p] = await Promise.all([s.delegations(), api<PlatformProjects>("/api/projects", undefined, platformFetch).catch(() => null)]);
    const owned = (p?.projects ?? [])
      .filter((x) => !x.team || x.team.role === "owner")
      .map((x) => ({ id: x.id, name: x.team ? x.name : "Personal", owner: x.team ? `team:${x.team.id}` : `account:${s.account}` }));
    const out: Row[] = [];
    for (const x of owned) {
      const live = d.delegations.find((l) => l.delegation.project === x.id && l.delegation.owner === x.owner && l.delegation.key === d.key && !l.revoke);
      if (!live) continue;
      out.push({ id: x.id, name: x.name, delegation: live.delegation, delegationId: live.id, view: await s.runGrant(x.id).catch(() => null) });
    }
    setRows(out);
  };
  useEffect(() => void load().catch((e: Error) => setErr(e.message)), []);
  if (!rows?.length) return err ? <p class="control-error">{err}</p> : null;
  // Only a browser or a phone (one all the way back to the account's root)
  // signs or revokes a standing grant: not the desktop app, whose keys may
  // be a program's. Every hosted workspace refuses the rest too.
  const mine = s.canApprove("browser");
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
  // The live delegation as control says, and the one the platform's grant
  // names: a revoke covers the project's grants under each.
  const delegationsOf = (r: Row) => [r.delegationId, ...(r.view?.grant ? [r.view.grant.grant.delegation] : [])];
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
                      : ` · runs may start until ${g.grant.expires ? day(g.grant.expires) : "revoked"}${r.view?.renew ? ": renew it" : ""}`
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
                        onClick={act(r.id, async () => {
                          const [after] = r.view?.stale ? [] : await s.revokeRunGrants(r.id, [g.grant.delegation], false);
                          await s.signRunGrant(r.delegation, daysOf(r), after);
                        })}
                      >
                        {r.view?.stale ? "Sign again" : "Renew"}
                      </button>{" "}
                    </>
                  ) : null}
                  <label class="control-check dim">
                    <input type="checkbox" data-stop-running checked={!!stop[r.id]} onChange={(e) => setStop({ ...stop, [r.id]: (e.target as HTMLInputElement).checked })} /> also
                    stop the runs going
                  </label>{" "}
                  <button class="secondary danger small" disabled={!!busy || !mine} data-revoke-run-grant={r.id} onClick={act(r.id, () => s.revokeRunGrants(r.id, delegationsOf(r), !!stop[r.id]))}>
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
        <b>Allow runs</b> signs, with this device, a standing grant: for as long as you pick, the platform may start runs and instruct them on
        every cloud workspace of the project, running whatever command the project's runs start, with none of your devices awake. Each
        workspace checks the signature. Deploys still ask you every time. <b>Revoke</b> stops new runs; the ones going finish unless you stop
        them too. A workspace that's asleep when you revoke is told later; one that's never told keeps honouring the grant until it expires.
      </p>
      {mine ? null : (
        <p class="dim" data-run-grant-elsewhere>
          Sign or revoke these from a browser or a phone of yours: the desktop app doesn't.
        </p>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
