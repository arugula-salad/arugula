// The session menu's Project… (Arugula #1177): which project the session
// is in, the reader's others to move it to, and who will see it after.
// A project is a click: the line under the list says what moving there
// does (the one pointed at, typed or arrowed to), and each row says who
// sees its sessions, so it reads before the click on a phone too. Typing
// filters, and Enter moves, as the prompt this replaces did.

import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { PlatformProject, PlatformProjects, SessionId } from "../../shared/proto";
import { Modal } from "../../shared/ui/modal";
import { platformFetch } from "../../shared/maintenance";
import { getSignedIn } from "../signed-in";
import { movedSaid, projectRef, projectRows, teamNow, teamOf, whoLine, whoSees } from "./session-project";

/** Open it for `session`, in a host of its own that goes with it, listing
 * only `team`'s projects when given (the Share dialog's move, #1182). One
 * at a time. */
export function openSessionProject(client: Client, session: SessionId, team?: string) {
  if (getSignedIn()?.phase !== "ready") {
    return client.toast("Projects are read as you, through Arugula cloud: open this machine in Arugula cloud, signed in");
  }
  if (document.querySelector("[data-session-project-host]")) return;
  const host = document.createElement("div");
  host.dataset.sessionProjectHost = "";
  document.body.appendChild(host);
  const done = () => {
    render(null, host);
    host.remove();
  };
  render(<ProjectDialog client={client} session={session} team={team} done={done} />, host);
}

/** The people the session is shared with, by name: a share stays wherever it goes (#1161, decision 4). */
async function sharedWith(client: Client, session: SessionId): Promise<string[]> {
  const r = await client.request(OPS["acl.get"].method, opPath("acl.get")).catch(() => null);
  if (!r?.ok) return [];
  const v = await r.json<{ grants: { session: SessionId; principal: string; name: string }[] }>().catch(() => null);
  return (v?.grants ?? []).filter((g) => g.session === session && !g.principal.startsWith("link:") && !g.principal.startsWith("team:")).map((g) => g.name);
}

function ProjectDialog({ client, session, team, done }: { client: Client; session: SessionId; team?: string; done: () => void }) {
  const [all, setAll] = useState<PlatformProjects | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [shared, setShared] = useState<string[]>([]);
  const [q, setQ] = useState("");
  const [at, setAt] = useState<string | null>(null);
  useEffect(() => {
    platformFetch("/api/projects", { credentials: "same-origin" })
      .then(async (r) => {
        if (!r.ok) throw new Error(`Arugula cloud said ${r.status}`);
        setAll((await r.json()) as PlatformProjects);
      })
      .catch(() => setErr("Couldn't read your projects from Arugula cloud"));
    void sharedWith(client, session).then(setShared);
  }, []);
  const s = client.state?.sessions.find((x) => x.id === session);
  const name = s?.name ?? `$${session}`;
  const current = s?.project;
  const rows = all
    ? projectRows(all, current)
        .filter((p) => !team || teamOf(p)?.id === team)
        .filter((p) => !q.trim() || p.name.toLowerCase().includes(q.trim().toLowerCase()))
    : [];
  // Where Enter goes: the row pointed or arrowed at, else the first one typed for.
  const target = rows.find((p) => p.id === at) ?? (q.trim() ? rows.find((p) => p.id !== current?.id) : undefined) ?? null;
  const line = all ? whoLine(teamNow(all, current), current, target, shared) : null;
  const move = (to: PlatformProject) => {
    if (to.id === current?.id) return;
    done();
    client.intent({ op: "set_session_project", session, project: projectRef(to) });
    client.toast(movedSaid(name, to));
  };
  const step = (by: number) => {
    if (!rows.length) return;
    const i = rows.findIndex((p) => p.id === target?.id);
    setAt(rows[i === -1 ? (by > 0 ? 0 : rows.length - 1) : (i + by + rows.length) % rows.length].id);
  };
  return (
    <Modal
      close={done}
      title={`Which project is “${name}” in?`}
      root={{ "data-session-project-dialog": true }}
      onSubmit={(e) => {
        e.preventDefault();
        if (target) move(target);
      }}
      buttons={
        <button class="secondary" type="button" onClick={done}>
          Cancel
        </button>
      }
    >
      <p class="dim">A session is in one project. Its project decides where it shows and who can see it.</p>
      {err ? (
        <p data-session-project-error>{err}</p>
      ) : !all ? (
        <p class="dim">Reading your projects…</p>
      ) : (
        <>
          <input
            data-session-project-filter
            aria-label="Find a project"
            placeholder="project"
            autofocus
            value={q}
            onInput={(e) => {
              setQ((e.target as HTMLInputElement).value);
              setAt(null);
            }}
            onKeyDown={(e) => {
              if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
              e.preventDefault();
              step(e.key === "ArrowDown" ? 1 : -1);
            }}
          />
          <div class="prompt-choices session-project-rows" role="group" aria-label="Projects" onPointerLeave={() => setAt(null)}>
            {rows.map((p) => {
              const now = p.id === current?.id;
              return (
                <button
                  type="button"
                  key={p.id}
                  data-session-project-row={p.id}
                  data-now={now || undefined}
                  class={[now && "strong", p.id === target?.id && "active"].filter(Boolean).join(" ") || undefined}
                  disabled={now}
                  onPointerEnter={() => setAt(p.id)}
                  onFocus={() => setAt(p.id)}
                  onClick={() => move(p)}
                >
                  {p.name}
                  {now && <span class="dim"> (now)</span>}
                  <span class="dim"> · {whoSees(p)}</span>
                </button>
              );
            })}
            {!rows.length && <span class="dim" data-session-project-none>No project of yours {q.trim() ? "has that name" : "is there to pick"}.</span>}
          </div>
          {line && (
            <p class={line.warn ? "session-project-who warn" : "session-project-who"} data-session-project-who data-warn={line.warn || undefined}>
              {line.text}
            </p>
          )}
        </>
      )}
    </Modal>
  );
}
