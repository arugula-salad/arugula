// Runs on hosted workspaces (M211), under Hosted machines in Devices and
// machines: for each project this account owns that has a delegation, its
// standing grant ("One standing grant per project starts runs"). Allow
// runs signs it with this device (here or on the phone), so the platform
// can start runs on the project's hosted workspaces with every device
// asleep; Revoke signs its revoke, and can stop the runs going too. The
// platform keeps both; the hosted machines check them.

import { useEffect, useState } from "preact/hooks";
import { api, type ControlSession, type RunGrantView } from "../control";
import type { Delegation } from "../e2e/hosted.ts";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProjects } from "../../shared/proto.gen";

interface Row {
  id: string;
  name: string;
  delegation: Delegation;
  view: RunGrantView | null;
}

const day = (ms: number) => new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

export function RunGrants({ s }: { s: ControlSession }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [busy, setBusy] = useState("");
  const [stop, setStop] = useState<Record<string, boolean>>({});
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
      out.push({ id: x.id, name: x.name, delegation: live.delegation, view: await s.runGrant(x.id).catch(() => null) });
    }
    setRows(out);
  };
  useEffect(() => void load().catch((e: Error) => setErr(e.message)), []);
  if (!rows?.length) return err ? <p class="control-error">{err}</p> : null;
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
  return (
    <>
      <h3 class="control-group">Runs on hosted workspaces</h3>
      <ul class="control-devices" data-run-grants>
        {rows.map((r) => {
          const g = r.view?.grant;
          return (
            <li key={r.id} data-project={r.id} data-run-grant={g ? "yes" : r.view?.offer ? "offered" : "no"}>
              <span>
                {r.name}
                <span class="dim">
                  {g
                    ? ` · runs may start with your devices asleep, since ${day(g.grant.made)}${g.grant.expires ? `, until ${day(g.grant.expires)}` : ""}`
                    : r.view?.offer
                      ? " · has hosted workspaces: allow runs to start on them"
                      : " · no runs"}
                </span>
              </span>
              {g ? (
                <span>
                  <label class="dim">
                    <input
                      type="checkbox"
                      data-stop-running
                      checked={!!stop[r.id]}
                      onChange={(e) => setStop({ ...stop, [r.id]: (e.target as HTMLInputElement).checked })}
                    />{" "}
                    also stop the runs going
                  </label>{" "}
                  <button class="control-revoke quiet-danger" disabled={!!busy} data-revoke-run-grant={r.id} onClick={act(r.id, () => s.revokeRunGrant(g.grant, !!stop[r.id]))}>
                    Revoke
                  </button>
                </span>
              ) : (
                <button disabled={!!busy} data-sign-run-grant={r.id} onClick={act(r.id, () => s.signRunGrant(r.delegation))}>
                  Allow runs
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <p class="dim control-explain">
        <b>Allow runs</b> signs, with this device, that the platform may start and instruct runs on the project's hosted workspaces, so
        scheduled runs start while your devices sleep. Each workspace checks the signature. Deploys still ask you every time. <b>Revoke</b> stops
        new runs; the ones going finish unless you stop them too.
      </p>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
