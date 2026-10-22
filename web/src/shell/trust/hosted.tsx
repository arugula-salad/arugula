// Hosted machines in Devices and machines (M178): the ones this browser
// accepted for a project, labelled "Hosted: the operator can read", and,
// for each project this account owns, whether control may enroll them
// (the project delegation, signed here by this device).

import { useEffect, useState } from "preact/hooks";
import { api, type ControlSession } from "../control";
import type { Delegation, DelegationRevoke } from "../e2e/hosted.ts";
import { machineState } from "../ui/hosts";
import { platformFetch } from "../../shared/maintenance";
import { HOSTED_LABEL } from "../../shared/hosted";
import type { PlatformProjects } from "../../shared/proto.gen";

interface Listed {
  key: string;
  delegations: { id: string; delegation: Delegation; revoke: DelegationRevoke | null }[];
  projects: { id: string; name: string; owner: string }[];
}

export function HostedMachines({ s }: { s: ControlSession }) {
  const [list, setList] = useState<Listed | null>(null);
  const [busy, setBusy] = useState("");
  const [err, setErr] = useState("");
  const load = async () => {
    const [d, p] = await Promise.all([s.delegations(), api<PlatformProjects>("/api/projects", undefined, platformFetch).catch(() => null)]);
    // The projects this account owns: its own, and its teams' as an owner.
    const projects = (p?.projects ?? [])
      .filter((x) => !x.team || x.team.role === "owner")
      .map((x) => ({ id: x.id, name: x.team ? x.name : "Personal", owner: x.team ? `team:${x.team.id}` : `account:${s.account}` }));
    setList({ ...d, projects });
  };
  useEffect(() => void load().catch((e: Error) => setErr(e.message)), []);
  const hosted = s.daemons.filter((d) => d.hosted);
  if (!hosted.length && !list?.projects.length) return null;
  const live = (p: Listed["projects"][number]) =>
    list?.delegations.find((x) => x.delegation.project === p.id && x.delegation.owner === p.owner && x.delegation.key === list.key && !x.revoke);
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
      <h3 class="control-group">Hosted machines</h3>
      <p class="dim">
        Cloud workspaces run Arugula in a box Arugula's operator runs, so they're labelled <b>{HOSTED_LABEL}</b>: whoever runs a VM can read
        its memory. Only a project's people reach its hosted machines.
      </p>
      {hosted.length ? (
        <ul class="control-devices" data-hosted-machines>
          {hosted.map((d) => (
            <li key={d.id} data-device={d.id} data-hosted>
              <span>
                {d.name}
                <span class="dim">
                  {" · "}
                  {machineState(d.name)}
                </span>
                <span class="control-badge" data-hosted-label>
                  {HOSTED_LABEL}
                </span>
              </span>
              <span />
            </li>
          ))}
        </ul>
      ) : null}
      {list?.projects.length ? (
        <ul class="control-devices" data-delegations>
          {list.projects.map((p) => {
            const d = live(p);
            return (
              <li key={p.id} data-project={p.id} data-delegated={d ? "yes" : "no"}>
                <span>
                  {p.name}
                  <span class="dim">{d ? " · control may enroll hosted machines into it" : " · no hosted machines"}</span>
                </span>
                {d ? (
                  <button class="control-revoke quiet-danger" disabled={!!busy} onClick={act(p.id, () => s.undelegate(d.delegation))}>
                    Stop
                  </button>
                ) : (
                  <button disabled={!!busy} data-delegate={p.id} onClick={act(p.id, () => s.delegate(p.id, p.owner))}>
                    Allow
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      ) : null}
      {list?.projects.length ? (
        <p class="dim control-explain">
          <b>Allow</b> signs, with this device, that control may enroll hosted machines into the project until you stop it. Stopping ends new
          ones; the ones there stay until they're removed.
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
