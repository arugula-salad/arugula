// Which cloud workspace a project's agent sessions run on, in its settings'
// Agents (`agents_workspace`, a key only a project sets: no owner has a
// default for it). It was a row of the project's Overrides until those
// went (Arugula #1323); the owner's Project defaults lists it too. A
// select, sent as it's chosen (`PUT /api/projects/{project}/settings`).
// Nothing while the project has no workspace and none is chosen.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { Section } from "../../shared/ui/settings-section";
import { runs, type HostedWorkspace } from "../runs";
import type { PlatformProject } from "../session";
import { choicesWith, optionOf, ownOf, settingOf, valueOf, words, type EffectiveSetting, type Value } from "./overrides";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

export function AgentWorkspace({ p }: { p: PlatformProject }) {
  const [own, setOwn] = useState<Value | undefined>(undefined);
  const [spaces, setSpaces] = useState<HostedWorkspace[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const path = `/api/projects/${encodeURIComponent(p.id)}/settings`;
  useEffect(() => {
    let live = true;
    call<EffectiveSetting[]>("GET", path).then(
      (list) => live && setOwn(ownOf(settingOf(list, "agents_workspace"), "project")),
      () => {},
    );
    void runs.workspaces(p.id).then((w) => live && setSpaces(w));
    return () => void (live = false);
  }, [path]);
  if (own === undefined || (own === null && !spaces.length)) return null;
  const changes = !p.team || p.team.role !== "viewer";
  const name = (id: string) => spaces.find((w) => w.id === id)?.name;
  const choose = async (v: Value) => {
    setBusy(true);
    setErr("");
    try {
      await call("PUT", path, { key: "agents_workspace", value: v });
      setOwn(v);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section id="agent-workspace" title="Agent workspace" summary={words("agents_workspace", own, name)} lede="The cloud workspace its agent sessions run on.">
      <div class="platform-link-repo" data-agent-workspace>
        <select
          class="field"
          aria-label="The workspace agent sessions run on"
          value={optionOf(own)}
          disabled={!changes || busy}
          data-agent-workspace-select
          onChange={(e) => void choose(valueOf("agents_workspace", e.currentTarget.value))}
        >
          <option value="">{words("agents_workspace", null)}</option>
          {choicesWith(spaces.map((w) => w.id), own).map((v) => (
            <option key={String(v)} value={optionOf(v)}>
              {words("agents_workspace", v, name)}
            </option>
          ))}
        </select>
        {err && (
          <span class="control-error" role="alert">
            {err}
          </span>
        )}
      </div>
      {!changes && <p class="dim">A team's owners and editors choose it.</p>}
    </Section>
  );
}
