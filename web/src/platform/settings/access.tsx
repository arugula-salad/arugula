// A project's Access view (Arugula #682), the first section of its
// settings, for every member: who can do what here, in one place, read
// from `GET /api/projects/{id}/grants` and changed nowhere on this page.
// Five parts, each saying whose word it is, so nobody reads control's copy
// as a machine's decision: the project roles (the team's roster as
// control's copy stands; each machine checks the roster itself); whose
// machines drive the project's cloud workspaces, one row per member, by
// `cloud.drive` given on join and never revoked per project; the powers
// over the project (every live grant, machines' and agent tokens', whose
// each is and why it does nothing if it doesn't; a giver who left is a
// former member, unnamed; ids only for an owner, who may revoke); what an
// owner allowed (the delegation for cloud workspaces on control, the
// standing run grant and its expiry, confirmed with a passkey, stale when
// it names an earlier delegation) and its cloud workspaces, where an
// editor has full use as an owner does. A platform from before the route
// answers 404, and the section says so instead.

import { useEffect, useState } from "preact/hooks";
import { agentLabel } from "../../shared/agent-label";
import { platformFetch } from "../../shared/maintenance";
import { Time } from "../../shared/ui/time";
import type { PlatformProject } from "../session";
import { Section } from "../../shared/ui/settings-section";

/** Someone named in the view; `former`: they left the project, and the
 * view doesn't name them. */
type Who = { account: string | null; name: string | null; former?: boolean };

/** The view, as the route answers it. */
export interface AccessView {
  project: { id: string; name: string; owner: { kind: "team" | "account"; id: string; name?: string | null; locked?: boolean } };
  role: "owner" | "editor" | "viewer";
  roles: { says: string; members: { account: string; name: string; role: string }[] };
  /** One row per member: how many of their machines drive the project's
   * cloud workspaces by `cloud.drive`, as their role allows. */
  cloud_drive: { says: string; members: { owner: Who; role: string; power: string; machines: number; driving: number }[] };
  powers: {
    says: string;
    grants: {
      /** A project owner's, to revoke; null for everyone else. */
      id: string | null;
      subject: { kind: "daemon" | "agent"; id: string | null; name: string | null };
      owner: Who;
      owner_role: string | null;
      power: string;
      created_at: number;
      /** Why it does nothing now (the grants list's definition for a
       * write on a viewer's; a giver who left; a machine that isn't its
       * giver's), else null. */
      idle: string | null;
    }[];
  };
  allow: {
    says: string;
    delegation: { live: boolean; id: string | null } | null;
    run_grant: { id: string; made: number; expires: number | null; account: string | null; created_at: number; stale: boolean } | null;
  };
  workspaces: { says: string; full_use: string; list: { id: string; name: string; state: string; created_by: Who | null; created_at: number; full_use: string }[] };
}

const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

const who = (w: Who | null | undefined) => (w ? (w.former ? "a former member" : w.name || w.account || "someone") : "someone");

/** A member's cloud.drive row: how many of their machines drive. */
export function driversLine(m: AccessView["cloud_drive"]["members"][number]): string {
  const role = ROLE[m.role]?.toLowerCase() ?? m.role;
  if (m.machines === 0) return "no machine joined";
  if (m.machines === m.driving) return `${m.machines} machine${m.machines === 1 ? "" : "s"} drive as an ${role}`;
  return `${m.driving} of ${m.machines} machines drive as an ${role}; the rest had cloud.drive revoked by their owner`;
}

export function Access({ p }: { p: PlatformProject }) {
  const [v, setV] = useState<AccessView | null>(null);
  const [err, setErr] = useState("");
  // A platform from before the view: say so, don't show an error.
  const [old, setOld] = useState(false);
  useEffect(() => {
    let live = true;
    setV(null);
    setErr("");
    setOld(false);
    platformFetch(`/api/projects/${encodeURIComponent(p.id)}/grants`, { credentials: "same-origin" })
      .then(async (r) => {
        const j = (await r.json().catch(() => ({}))) as { error?: string };
        if (!live) return;
        if (r.status === 404 && j.error !== "no such project") return setOld(true);
        if (!r.ok) throw new Error(j.error ?? `Request failed (${r.status})`);
        setV(j as AccessView);
      })
      .catch((e: Error) => live && setErr(e.message));
    return () => void (live = false);
  }, [p.id]);
  const grants = v?.powers.grants ?? [];
  return (
    <Section
      id="access"
      title="Access"
      count={v ? grants.length : undefined}
      hooks={{ "data-access": v ? "yes" : old ? "old" : "" }}
      lede="Who can do what here: project roles, whose machines drive its cloud workspaces, the powers members' agents hold, what an owner allowed, and the cloud workspaces. Read-only."
      more={
        <>
          <p>
            A <b>project role</b> is the team's role, from the roster: an owner, an editor or a viewer. Every machine joined to an account holds
            <code>cloud.drive</code> over it from its join, so its agents drive this project's cloud workspaces as its owner's role allows; its owner
            revokes that per machine, nobody per project. A <b>power</b> is what a machine's agents or an agent token may do here, given by its owner
            under Settings, Agents, and never more than its owner reads; a row says when it does nothing now. <b>Allow</b> is what a project owner
            signed: cloud workspaces for the project, and runs on them with every device asleep. On a cloud workspace an editor has full use, as an
            owner does; a viewer watches.
          </p>
          <p>Each part says whose word it is. Nothing here changes what's enforced: roles change on your machines page, powers under Settings.</p>
        </>
      }
    >
      {old && (
        <p class="dim" data-access-old>
          Control's platform doesn't list a project's access yet: it's older than this page. Try again once it's updated.
        </p>
      )}
      {err && <p class="control-error">{err}</p>}
      {v && (
        <>
          <h3>Project roles</h3>
          <ul class="platform-list settings-rows" data-access-roles>
            {v.roles.members.map((m) => (
              <li key={m.account} data-access-role={m.account}>
                <span>{m.name || m.account}</span>
                <span class="settings-tag">{ROLE[m.role] ?? m.role}</span>
              </li>
            ))}
          </ul>
          {v.project.owner.locked && <p class="settings-warn">The team is locked: only its owners get in while it is.</p>}
          <p class="dim settings-note">By {v.roles.says}.</p>

          <h3>Cloud workspaces' drivers</h3>
          <ul class="platform-list settings-rows" data-access-drivers>
            {v.cloud_drive.members.map((m) => (
              <li key={m.owner.account ?? m.role} data-access-driver={m.owner.account ?? ""}>
                <span>{who(m.owner)}'s machines</span>
                <span class="settings-tag">{m.power}</span>
                <span class="dim">{driversLine(m)}</span>
              </li>
            ))}
          </ul>
          <p class="dim settings-note">By {v.cloud_drive.says}.</p>

          <h3>Other powers</h3>
          {grants.length ? (
            <table class="settings-table" data-access-powers>
              <thead>
                <tr>
                  <th scope="col">Whose agent</th>
                  <th scope="col">Power</th>
                  <th scope="col">Since</th>
                </tr>
              </thead>
              <tbody>
                {grants.map((g) => (
                  <tr key={g.id ?? `${g.subject.kind}:${g.subject.name}:${g.power}:${g.created_at}`} data-access-power={g.power} data-access-subject={g.subject.id ?? ""}>
                    <th scope="row">
                      {agentLabel(who(g.owner), { machine: g.subject.id ?? (g.subject.kind === "agent" ? "agt_" : ""), machine_name: g.subject.name ?? undefined })}
                      {g.owner_role && <span class="settings-tag">{ROLE[g.owner_role] ?? g.owner_role}</span>}
                    </th>
                    <td data-col="Power">
                      <code>{g.power}</code>
                      {g.idle && (
                        <span class="settings-tag" data-access-idle>
                          {g.idle}
                        </span>
                      )}
                    </td>
                    <td data-col="Since">
                      <Time at={g.created_at} on />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <p class="dim" data-access-powers-none>
              No other power: no machine's or agent token's agents hold one here.
            </p>
          )}
          <p class="dim settings-note">By {v.powers.says}.</p>

          <h3>Allowed</h3>
          <ul class="platform-list settings-rows" data-access-allow>
            <li data-access-delegation={v.allow.delegation ? (v.allow.delegation.live ? "yes" : "no") : "unknown"}>
              <span>Cloud workspaces</span>
              <span class="dim">
                {v.allow.delegation === null
                  ? "control couldn't say whether an owner allowed them"
                  : v.allow.delegation.live
                    ? "allowed: an owner's device delegated this project to control"
                    : "not allowed: no live delegation"}
              </span>
            </li>
            <li data-access-run-grant={v.allow.run_grant ? (v.allow.run_grant.stale ? "stale" : "yes") : "no"}>
              <span>Runs with every device asleep</span>
              <span class="dim">
                {v.allow.run_grant?.stale ? (
                  "not now: the standing grant names an earlier delegation, so workspaces enrolled since refuse it; an owner signs it again"
                ) : v.allow.run_grant ? (
                  <>
                    allowed by {v.allow.run_grant.account ?? "an owner"} <Time at={v.allow.run_grant.made} on />
                    {v.allow.run_grant.expires ? (
                      <>
                        , expires <Time at={v.allow.run_grant.expires} />
                      </>
                    ) : (
                      ", until it's revoked"
                    )}
                  </>
                ) : (
                  "not allowed: no live standing grant"
                )}
              </span>
            </li>
          </ul>
          <p class="dim settings-note">{v.allow.says[0].toUpperCase() + v.allow.says.slice(1)}.</p>

          <h3>Cloud workspaces</h3>
          {v.workspaces.list.length ? (
            <ul class="platform-list settings-rows" data-access-workspaces>
              {v.workspaces.list.map((w) => (
                <li key={w.id} data-access-workspace={w.name}>
                  <span>{w.name}</span>
                  <span class="settings-tag">{w.state}</span>
                  <span class="dim">
                    {w.full_use}
                    {w.created_by ? `; made by ${who(w.created_by)}` : ""}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p class="dim" data-access-workspaces-none>
              None. On one, {v.workspaces.full_use}.
            </p>
          )}
          <p class="dim settings-note">By {v.workspaces.says}.</p>
        </>
      )}
    </Section>
  );
}
