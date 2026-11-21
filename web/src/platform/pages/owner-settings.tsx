// An owner's settings (Arugula #897): Settings' Team and billing scope
// (Arugula #1104), a team's at `/settings/team/:team`, Personal's at
// `/settings/team/personal` (they were `/t/:team/settings` and
// `/settings/personal`). Its sections, as every scope's are, in a flat list.
// First the owner's own: Members, Machines (../settings/owner-machines), Chat
// (the team chat's owners-only switches), Plan and billing, and Labs. Then
// what every project starts with: its default project, and Project
// defaults. Members and Machines change in place on control's word
// (aru-141), and Labs' flags are control's Teams panel's list (Arugula
// #1118), read and set through the app's host. What takes a passkey
// (making or removing an owner), and what the hub doesn't answer on this
// host yet, is a ↗ row in place, opening control's page (#1106).
//
// Project defaults is a table: a row per key a project overrides, the
// owner's default first, then a column per project that isn't archived,
// sideways when it's wide with the key and the default kept in place.
// Every cell is a select, and a change is staged (../settings/overrides):
// a bar above the table counts them, Save sends one PUT each and says
// which failed, Cancel drops them, and leaving asks first. A team's
// viewers read it; only its owners choose its default project.

import { useEffect, useState } from "preact/hooks";
import type { ProjectMember, SessionSettings } from "../../shared/proto";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import { LabsFlagList, PUSHED, type LabsFlags } from "../../shared/ui/labs-flags";
import { Chip } from "../../shared/ui/state-chip";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { changesOwners, expiresIn, makeInvite, nextRoster, word, type MadeInvite, type Member, type PresignedInvite, type TeamRole } from "../../shared/team-roster";
import { useHoldLeaving } from "../router";
import { runs, type HostedWorkspace } from "../runs";
import { controlOrigin, session, type PlatformProject } from "../session";
import { owners } from "../owners";
import { TeamChat } from "../settings/chat-alerts";
import { ControlRow, OwnerMachines } from "../settings/owner-machines";
import { PlanAndBilling } from "../settings/plan";
import { hubCall, reloadTeams, useTeams, type HubTeam } from "../settings/teams";
import {
  ISSUE_HOMES,
  KEYS,
  OWNER,
  STALL_MINUTES,
  changes,
  choicesWith,
  defaultOf,
  infoOf,
  optionOf,
  ownerCell,
  ownerOf,
  ownOf,
  projectCell,
  send,
  settingOf,
  stage,
  valueOf,
  words,
  writes,
  type Cell,
  type EffectiveSetting,
  type Key,
  type Layers,
  type Staged,
  type Value,
  type Write,
} from "../settings/overrides";
import { joined, plural } from "../settings/summary";

/** A call to the platform: its JSON, or its `{error}` thrown, with the status. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw Object.assign(new Error(v.error ?? `Request failed (${r.status})`), { status: r.status });
  return v as T;
}

const enc = encodeURIComponent;

/** The owner's projects that aren't archived, by name. */
function projectsOf(owner: string): PlatformProject[] {
  return session.projects.filter((p) => !p.archived && ownerOf(p).owner === owner).sort((a, b) => a.name.localeCompare(b.name));
}

/** The reader's role in the owner: Personal is theirs; a team's, as its
 * project says. */
function roleIn(owner: string): string | undefined {
  if (owner === "personal") return "owner";
  return session.projects.find((p) => p.team?.id === owner)?.team?.role;
}

export function OwnerSettings({ owner }: { owner: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const projects = projectsOf(owner);
  const team = owner === "personal" ? null : session.projects.find((p) => p.team?.id === owner)?.team ?? null;
  const name = owner === "personal" ? "Just you" : (team?.name ?? (projects[0] && ownerOf(projects[0]).name) ?? "Team");
  const role = roleIn(owner);
  if (owner !== "personal" && !team && projects.length === 0)
    return (
      <Section id="general" title="General" summary="Not one of yours">
        <p class="dim">That team isn't one of yours (or it's gone).</p>
      </Section>
    );
  return (
    <>
      {team && <Members owner={owner} name={name} role={role} members={team.members} />}
      <OwnerMachines owner={owner} name={name} role={role} teams={owners(session.projects).some((o) => o.kind === "team")} />
      {team && <TeamChat team={owner} name={name} role={role} />}
      <PlanAndBilling team={team ? owner : undefined} />
      <Labs owner={owner} name={name} />
      <General owner={owner} projects={projects} role={role} />
      <ProjectDefaults owner={owner} name={name} projects={projects} role={role} />
    </>
  );
}

const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

/** Members: the roster. Its owners change it in place, on control's word
 * (aru-141): invite links, roles and removing someone, and who waits on an
 * Ask me first link. Making or removing an owner takes the owner's passkey,
 * which only control's page asks for (aru-078), so that's a ↗ row there; so
 * is everything while the hub doesn't answer its teams on this host. */
function Members({ owner, name, role, members }: { owner: string; name: string; role: string | undefined; members: ProjectMember[] }) {
  const hub = useTeams();
  const t = hub && "teams" in hub ? hub.teams.find((x) => x.team === owner) : undefined;
  const names = t ? t.roster.members.map((m) => ({ account: m.account, name: t.names?.[m.account] ?? m.name, role: m.role })) : members;
  const here = role === "owner" && t?.role === "owner";
  const toControl = (label: string, why: string, hook: string) => (
    <ul class="platform-list settings-rows">
      <ControlRow href={`${controlOrigin()}/#teams`} label={label} why={why} hook={hook} />
    </ul>
  );
  return (
    <Section
      id="members"
      title="Members"
      summary={joined(plural(names.length, "person", "people"), plural(names.filter((m) => m.role === "owner").length, "owner"))}
      count={names.length}
      lede={`Who's in ${name}, and their role.`}
      hooks={{ "data-owner-members": owner }}
    >
      {here && t ? (
        <TeamRoster t={t} name={name} toControl={toControl} />
      ) : (
        <>
          <ul class="platform-list settings-rows">
            {names.map((m) => (
              <li key={m.account} data-owner-member={m.account}>
                <b>{m.name || m.account}</b>
                <Chip kind="tag">{ROLE[m.role] ?? m.role}</Chip>
              </li>
            ))}
          </ul>
          {role !== "owner" ? (
            <p class="dim">Its owners invite people and change roles.</p>
          ) : hub === null ? null : (
            <>
              {"err" in hub && <p class="control-error">{hub.err}</p>}
              {toControl("Invite people, remove them and change roles", "On your machines page, under Teams…", "members")}
            </>
          )}
        </>
      )}
    </Section>
  );
}

/** A team's roster as its owner changes it here. */
function TeamRoster({ t, name, toControl }: { t: HubTeam; name: string; toControl: (label: string, why: string, hook: string) => preact.JSX.Element }) {
  const me = session.me?.account;
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<Member | null>(null);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr("");
    try {
      await f();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  // The next version, on control's word. One that changes who the owners
  // are needs a passkey (DK7), which this page doesn't ask for.
  const change = (fn: (ms: Member[]) => Member[]) =>
    act(async () => {
      const next = nextRoster(t.roster, fn);
      if (changesOwners(next, t.roster)) throw new Error("Making or removing an owner is on your machines page.");
      await hubCall("POST", `/api/teams/${enc(t.team)}/roster`, { roster: { ...next, by: "", sig: "" } });
      reloadTeams();
      await session.load();
    });
  const who = (m: Member) => t.names?.[m.account] ?? m.name;
  return (
    <>
      <ul class="platform-list settings-rows" data-team-roster={t.team}>
        {t.roster.members.map((m) => (
          <li key={m.account} data-owner-member={m.account}>
            <b>
              {who(m)}
              {m.account === me ? " (you)" : ""}
            </b>
            <span class="settings-row-act">
              {m.role === "owner" || m.account === me ? (
                <Chip kind="tag">{ROLE[m.role] ?? m.role}</Chip>
              ) : (
                <>
                  <select
                    class="field"
                    value={m.role}
                    disabled={busy}
                    aria-label={`${who(m)}'s role`}
                    data-member-role={m.account}
                    onChange={(e) => {
                      const r = e.currentTarget.value as TeamRole;
                      void change((ms) => ms.map((x) => (x.account === m.account ? { ...x, role: r } : x)));
                    }}
                  >
                    <option value="editor">{ROLE.editor}</option>
                    <option value="viewer">{ROLE.viewer}</option>
                  </select>
                  <button class="secondary danger small" disabled={busy} data-remove-member={m.account} onClick={() => setRemoving(m)}>
                    Remove
                  </button>
                </>
              )}
            </span>
          </li>
        ))}
      </ul>
      {toControl("Make someone an owner, or remove an owner", "It takes your passkey, on your machines page", "owners")}
      <Waiting t={t} busy={busy} act={act} change={change} />
      <Invites t={t} name={name} />
      {err && <p class="control-error">{err}</p>}
      {removing && (
        <ConfirmRemove
          title={`Remove ${who(removing)} from ${name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            const gone = removing.account;
            setRemoving(null);
            void change((ms) => ms.filter((x) => x.account !== gone));
          }}
        >
          <p>They lose the team's machines at once. To come back, they need a new invite.</p>
        </ConfirmRemove>
      )}
    </>
  );
}

/** Who used an Ask me first link and waits for an owner's yes. Letting in
 * someone the link makes an owner changes who the owners are, so that one
 * is control's page's. */
function Waiting({ t, busy, act, change }: { t: HubTeam; busy: boolean; act: (f: () => Promise<unknown>) => Promise<void>; change: (fn: (ms: Member[]) => Member[]) => Promise<void> }) {
  const waiting = t.requests ?? [];
  if (!waiting.length) return null;
  return (
    <>
      <h3>Waiting to join</h3>
      <ul class="platform-list settings-rows" data-team-waiting={t.team}>
        {waiting.map((q) => (
          <li key={q.account} data-team-request={q.account}>
            <b>{q.name}</b>
            <span class="dim">as {ROLE[q.invite.role] ?? q.invite.role}</span>
            <span class="settings-row-act">
              {q.invite.role !== "owner" && (
                <button
                  class="primary small"
                  disabled={busy}
                  data-admit={q.account}
                  onClick={() =>
                    void change((ms) => [...ms.filter((m) => m.account !== q.account), { account: q.account, root: q.root, role: q.invite.role, name: word(q.name) }])
                  }
                >
                  Let in
                </button>
              )}
              <button
                class="secondary danger small"
                disabled={busy}
                data-reject={q.account}
                onClick={() =>
                  void act(async () => {
                    await hubCall("POST", `/api/teams/${enc(t.team)}/requests/${enc(q.account)}/reject`, {});
                    reloadTeams();
                  })
                }
              >
                Turn down
              </button>
            </span>
          </li>
        ))}
      </ul>
    </>
  );
}

/** Make an invite link, and the one-click links nobody has used yet, each
 * with Revoke. A link opens on control's page, which signs its person in;
 * its one-time key is only in the link, so a link can be copied only when
 * it's made. An owner invite asks first and is let in on control's page. */
function Invites({ t, name }: { t: HubTeam; name: string }) {
  const [role, setRole] = useState<TeamRole>("editor");
  const [askFirst, setAskFirst] = useState(false);
  const [made, setMade] = useState<MadeInvite | null>(null);
  const [unused, setUnused] = useState<PresignedInvite[]>([]);
  const [reload, setReload] = useState(0);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const base = `/api/teams/${enc(t.team)}`;
  useEffect(() => {
    let live = true;
    hubCall<{ invites: PresignedInvite[] }>("GET", `${base}/presigned`).then(
      (v) => live && setUnused(v.invites),
      () => live && setUnused([]),
    );
    return () => void (live = false);
  }, [base, t.roster.version, t.locked, made, reload]);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr("");
    try {
      await f();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <h3>Invite</h3>
      <form
        class="settings-add"
        data-invite={t.team}
        onSubmit={(e) => {
          e.preventDefault();
          void act(async () => setMade(await makeInvite({ team: t.team, role, askFirst, origin: controlOrigin(), post: (path, body) => hubCall("POST", path, body) })));
        }}
      >
        <label class="settings-field">
          Role
          <select class="field" value={role} data-invite-role={t.team} onChange={(e) => setRole(e.currentTarget.value as TeamRole)}>
            <option value="editor">{ROLE.editor}</option>
            <option value="viewer">{ROLE.viewer}</option>
          </select>
        </label>
        <label class="settings-field" title="Each person waits for an owner's yes">
          <input type="checkbox" checked={askFirst} data-invite-ask-first onChange={(e) => setAskFirst(e.currentTarget.checked)} /> Ask me first
        </label>
        <button class="primary" disabled={busy || t.locked} data-invite-make>
          Make a link
        </button>
      </form>
      {t.locked && <p class="dim">{name} is locked: unlock it on your machines page to invite people.</p>}
      {made && (
        <p data-invite-made>
          {made.why ? `${made.why}, so this link asks you first. ` : ""}
          {made.asks
            ? "Anyone with this link can ask to join (for a week); you approve each:"
            : "One person can join with this link, within a day, and you're told when they do. Send it only to them:"}
          <CopyText text={made.link} share data-invite-link />
        </p>
      )}
      {unused.length > 0 && (
        <>
          <p class="dim">One-click links nobody has used yet:</p>
          <ul class="platform-list settings-rows" data-presigned-list>
            {unused.map((i) => (
              <li key={i.key} data-presigned={i.key}>
                <span>
                  For {ROLE[i.role] ?? i.role}
                  {i.by === session.me?.account ? "" : `, from ${i.by_name}`}
                </span>
                <span class="dim">{expiresIn(i.expires)}</span>
                <span class="settings-row-act">
                  <button
                    class="secondary danger small"
                    disabled={busy}
                    data-cancel-presigned={i.key}
                    title="Nobody can join with this link any more"
                    onClick={() =>
                      void act(async () => {
                        // Already gone is gone.
                        await hubCall("DELETE", `${base}/presigned/${enc(i.key)}`).catch((e: { status?: number }) => {
                          if (e.status !== 404) throw e;
                        });
                        setReload((n) => n + 1);
                      })
                    }
                  >
                    Revoke
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {err && <p class="control-error">{err}</p>}
    </>
  );
}

/** Labs: the flags control sets for the owner's machines (Arugula #1118),
 * as control's Teams panel lists them: yours on Personal, a team's on its
 * own, which its owners set and everyone else reads. Only for someone with
 * a machine that has Developer settings (`dev`): strangers never see Labs.
 * A control that doesn't answer `/api/flags` on this host yet (a bare 404)
 * gets the ↗ row to its Teams panel. */
function Labs({ owner, name }: { owner: string; name: string }) {
  const team = owner !== "personal";
  const [kept, setKept] = useState<LabsFlags | null>(null);
  const [elsewhere, setElsewhere] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    platformFetch("/api/flags", { credentials: "same-origin" }).then(
      async (r) => {
        const v = (await r.json().catch(() => ({}))) as Partial<LabsFlags> & { error?: string };
        if (!live) return;
        if (r.status === 404 && !v.error) return setElsewhere(true);
        if (!r.ok) return setErr(v.error ?? `Request failed (${r.status})`);
        setKept(v as LabsFlags);
      },
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, []);
  const lede = team ? `Labs features turned on for ${name}'s machines.` : "Labs features turned on for every machine of yours.";
  if (elsewhere || err)
    return (
      <Section id="labs" title="Labs" summary={err ? "Couldn't load" : "On your machines page"} lede={lede} hooks={{ "data-owner-labs": owner }}>
        {err ? (
          <p class="control-error">{err}</p>
        ) : (
          <ul class="platform-list settings-rows">
            <ControlRow href={`${controlOrigin()}/#teams`} label={team ? `Labs for ${name}'s machines` : "Labs for your machines"} why="Only your machines page reaches these for now." hook="labs" />
          </ul>
        )}
      </Section>
    );
  const mine = kept?.dev ? (team ? kept.teams[owner] : { flags: kept.account, owner: true }) : undefined;
  if (!mine) return null;
  const set = async (flag: string, on: boolean | null) => {
    setErr("");
    try {
      const { flags } = await call<{ flags: Record<string, boolean> }>("POST", "/api/flags", { name: flag, on, team: team ? owner : undefined });
      setKept((k) => k && (team ? { ...k, teams: { ...k.teams, [owner]: { ...k.teams[owner], flags } } } : { ...k, account: flags }));
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  const named = PUSHED.filter((f) => mine.flags[f.name] !== undefined).length;
  return (
    <Section
      id="labs"
      title="Labs"
      summary={named ? `${named} set` : "None set"}
      lede={lede}
      more={<p>A machine's own Developer settings win over these, yours over a team's.{team && !mine.owner ? ` ${name}'s owners set its own.` : ""}</p>}
      hooks={{ "data-owner-labs": owner }}
    >
      <LabsFlagList
        scope={team ? name : "your machines"}
        what={
          !team
            ? "For every machine of yours. A machine's own Developer settings win over these, and these over a team's."
            : mine.owner
              ? "For the team's machines. A machine's own Developer settings win over these, and so do its owner's settings for their machines."
              : "For the team's machines, set by its owners. A machine's own Developer settings win over these."
        }
        flags={mine.flags}
        edit={mine.owner}
        set={(flag, on) => void set(flag, on)}
        open
      />
    </Section>
  );
}

/** General: the owner's name, and its default project, which only its
 * owners choose. One select and a Save, so a change isn't one click. */
function General({ owner, projects, role }: { owner: string; projects: PlatformProject[]; role: string | undefined }) {
  const [read, setRead] = useState<EffectiveSetting[] | null>(null);
  const [draft, setDraft] = useState<Value>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const path = `/api/owners/${enc(owner)}/settings`;
  const load = () =>
    call<EffectiveSetting[]>("GET", path).then(
      (v) => {
        setRead(v);
        setDraft(ownOf(settingOf(v, "default_project"), "owner"));
      },
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [owner]);
  const saved = read ? ownOf(settingOf(read, "default_project"), "owner") : null;
  const changes = role === "owner";
  const save = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setErr("");
    try {
      await call("PUT", path, { key: "default_project", value: draft });
      await load();
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const nameOf = (id: string) => session.project(id)?.name;
  return (
    <Section
      id="general"
      title="Default project"
      summary={read ? `Default project: ${words("default_project", saved, nameOf)}` : err ? "Couldn't load" : undefined}
      lede="Its default project: where its new things go when nothing says which.">
      {read && (
        <form class="settings-row" onSubmit={save} data-default-project-form>
          <label>
            Default project{" "}
            {changes ? (
              <select class="field" value={optionOf(draft)} disabled={busy} data-default-project onChange={(e) => setDraft(e.currentTarget.value || null)}>
                <option value="">None</option>
                {choicesWith(
                  projects.map((p) => p.id),
                  draft,
                ).map((id) => (
                  <option key={String(id)} value={String(id)}>
                    {words("default_project", id, nameOf)}
                  </option>
                ))}
              </select>
            ) : (
              <b data-default-project>{words("default_project", saved, nameOf)}</b>
            )}
          </label>
          {changes && (
            <button type="submit" class="secondary small" data-default-project-save disabled={busy || draft === saved}>
              Save
            </button>
          )}
        </form>
      )}
      {read && !changes && <p class="dim">Its owners choose it.</p>}
      {err && <p class="control-error">{err}</p>}
    </Section>
  );
}

/** Project defaults: the table, its staged changes and the bar above it. */
function ProjectDefaults({ owner, name, projects, role }: { owner: string; name: string; projects: PlatformProject[]; role: string | undefined }) {
  const [layers, setLayers] = useState<Layers | null>(null);
  const [err, setErr] = useState("");
  const [staged, setStaged] = useState<Staged>({});
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<{ write: Write; error: string }[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [spaces, setSpaces] = useState<Record<string, HostedWorkspace[]>>({});
  const ids = projects.map((p) => p.id);
  const key = ids.join();
  const load = async () => {
    const [own, ...each] = await Promise.all([
      call<EffectiveSetting[]>("GET", `/api/owners/${enc(owner)}/settings`),
      ...ids.map((id) => call<EffectiveSetting[]>("GET", `/api/projects/${enc(id)}/settings`).catch(() => [] as EffectiveSetting[])),
    ]);
    setLayers({ owner: own, projects: Object.fromEntries(ids.map((id, i) => [id, each[i]])) });
  };
  useEffect(() => {
    let live = true;
    load().catch((e: Error & { status?: number }) => live && setErr(e.status === 404 ? `You're not in ${name}.` : e.message));
    // The models agents may use, as a project's agent model lists them.
    if (ids[0]) call<SessionSettings>("GET", `/api/projects/${enc(ids[0])}/sessions/settings`).then((v) => live && setModels(v.models), () => {});
    // Each project's Cloud workspaces, for its agent workspace.
    void Promise.all(ids.map((id) => runs.workspaces(id))).then((ws) => live && setSpaces(Object.fromEntries(ids.map((id, i) => [id, ws[i]]))));
    return () => void (live = false);
  }, [owner, key]);
  const n = changes(staged);
  useHoldLeaving(n > 0);
  const writer = role !== undefined && role !== "viewer";
  const set = (scope: string, k: Key, v: Value) => layers && setStaged(stage(staged, layers, scope, k, v));
  const save = async () => {
    setBusy(true);
    const r = await send(writes(staged, owner, ids), (w) => call("PUT", w.path, w.body));
    setFailed(r.failed);
    try {
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
    setStaged(r.left);
    setBusy(false);
  };
  const cancel = () => {
    setStaged({});
    setFailed([]);
  };
  const workspaceName = (id: string) => Object.values(spaces).flat().find((w) => w.id === id)?.name;
  const projectName = (id: string) => session.project(id)?.name ?? id;
  const choices = (k: Key, project?: string): Value[] => {
    if (k === "issue_home") return ISSUE_HOMES;
    if (k === "agents_model") return models;
    if (k === "agents_workspace") return (project ? spaces[project] : [])?.map((w) => w.id) ?? [];
    return STALL_MINUTES;
  };
  const head = owner === "personal" ? "Your default" : "Team default";
  return (
    <Section
      id="defaults"
      title="Project defaults"
      summary={layers ? defaultsSummary(layers, projects) : err ? "Couldn't load" : undefined}
      lede={`What ${owner === "personal" ? "your projects use" : `every project in ${name} uses`} unless it overrides it.`}
      more={<p>A highlighted cell is a project's own value. Choose Default in it to follow the default again. Changes wait for Save.</p>}
      hooks={{ "data-project-defaults": owner }}
    >
      {err && <p class="control-error">{err}</p>}
      {layers && (
        <>
          {n > 0 && (
            <div class="defaults-bar" role="status" data-defaults-bar>
              <span data-defaults-count>{n === 1 ? "1 change" : `${n} changes`}</span>
              <button class="secondary small" disabled={busy} data-defaults-cancel onClick={cancel}>
                Cancel
              </button>
              <button class="primary small" disabled={busy} data-defaults-save onClick={() => void save()}>
                {busy ? "Saving…" : "Save"}
              </button>
            </div>
          )}
          {failed.length > 0 && (
            <div class="control-error" role="alert" data-defaults-failed>
              <p>Some changes weren't saved. They're still here to try again:</p>
              <ul>
                {failed.map((f) => (
                  <li key={`${f.write.scope}/${f.write.key}`}>
                    {f.write.scope === OWNER ? head : projectName(f.write.scope)}, {infoOf(f.write.key).name}: {f.error}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {projects.length === 0 && <p class="dim">No projects yet: the defaults count for each one it makes.</p>}
          <div class="defaults-scroll">
            <table class="settings-table defaults-table" data-defaults-table>
              <thead>
                <tr>
                  <th scope="col">Setting</th>
                  <th scope="col">{head}</th>
                  {projects.map((p) => (
                    <th scope="col" key={p.id}>
                      {p.name}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {KEYS.map((info) => {
                  const own = ownerCell(layers, staged, info.key);
                  return (
                    <tr key={info.key} data-defaults-key={info.key}>
                      <th scope="row">{info.name}</th>
                      <td class={cellClass(own)} data-defaults-cell="owner">
                        {info.owner && writer ? (
                          <CellSelect
                            label={`${info.name}, ${head.toLowerCase()}`}
                            k={info.key}
                            cell={own}
                            first={`Built-in (${words(info.key, info.builtin)})`}
                            choices={choices(info.key)}
                            name={workspaceName}
                            disabled={busy}
                            set={(v) => set(OWNER, info.key, v)}
                          />
                        ) : (
                          <span class={info.owner ? undefined : "dim"} title={info.owner ? undefined : "Each project chooses its own."}>
                            {words(info.key, own.shown, workspaceName)}
                          </span>
                        )}
                      </td>
                      {projects.map((p) => {
                        const c = projectCell(layers, staged, p.id, info.key);
                        const below = defaultOf(layers, staged, info.key);
                        return (
                          <td key={p.id} class={cellClass(c)} data-defaults-cell={p.id}>
                            {writer ? (
                              <CellSelect
                                label={`${info.name}, ${p.name}`}
                                k={info.key}
                                cell={c}
                                first={`Default (${words(info.key, below, workspaceName)})`}
                                choices={choices(info.key, p.id)}
                                name={workspaceName}
                                disabled={busy}
                                set={(v) => set(p.id, info.key, v)}
                              />
                            ) : (
                              <span>{c.overridden ? words(info.key, c.shown, workspaceName) : `Default (${words(info.key, c.shown, workspaceName)})`}</span>
                            )}
                          </td>
                        );
                      })}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          {!writer && <p class="dim">{name}'s owners and editors change these.</p>}
        </>
      )}
    </Section>
  );
}

/** What the list says of the defaults: how many are set, and how many
 * projects set their own. */
function defaultsSummary(layers: Layers, projects: PlatformProject[]): string {
  const set = KEYS.filter((info) => ownerCell(layers, {}, info.key).overridden).length;
  const own = projects.filter((p) => KEYS.some((info) => projectCell(layers, {}, p.id, info.key).overridden)).length;
  return joined(set ? `${set} set here` : "Built-in defaults", own > 0 && `${plural(own, "project")} set their own`);
}

const cellClass = (c: Cell) => ["defaults-cell", c.overridden && "defaults-own", c.staged && "defaults-staged"].filter(Boolean).join(" ");

/** A cell's select: the layer below first (its own value cleared), then
 * each choice. */
function CellSelect({
  label,
  k,
  cell,
  first,
  choices,
  name,
  disabled,
  set,
}: {
  label: string;
  k: Key;
  cell: Cell;
  first: string;
  choices: Value[];
  name: (id: string) => string | undefined;
  disabled: boolean;
  set: (v: Value) => void;
}) {
  return (
    <select
      class="field"
      aria-label={label}
      title={cell.staged ? "Not saved yet" : undefined}
      value={optionOf(cell.own)}
      disabled={disabled}
      onChange={(e) => set(valueOf(k, e.currentTarget.value))}
    >
      <option value="">{first}</option>
      {choicesWith(choices, cell.own).map((v) => (
        <option key={String(v)} value={optionOf(v)}>
          {words(k, v, name)}
        </option>
      ))}
    </select>
  );
}
