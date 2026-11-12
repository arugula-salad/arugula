// A project's Overrides (Arugula #897), in its settings: each key its owner
// sets a default for, as "Using team default: …" or "Overridden: … ·
// Reset", with a select to choose its own. Changes are staged as the
// owner's Project defaults stages them (./overrides): Reset and the select
// change only what's shown, and a bar counts them with Cancel and Save.
// Leaving with changes not saved asks first. A team's viewers read it.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { SessionSettings } from "../../shared/proto";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { follow, pathOf, useHoldLeaving } from "../router";
import { runs, type HostedWorkspace } from "../runs";
import type { PlatformProject } from "../session";
import {
  ISSUE_HOMES,
  KEYS,
  STALL_MINUTES,
  changes,
  choicesWith,
  defaultOf,
  optionOf,
  overrideLine,
  ownerOf,
  projectCell,
  send,
  settingOf,
  stage,
  valueOf,
  words,
  writes,
  type EffectiveSetting,
  type Key,
  type Layers,
  type Staged,
  type Value,
  type Write,
} from "./overrides";

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

const enc = encodeURIComponent;

export function Overrides({ p }: { p: PlatformProject }) {
  const { owner, name } = ownerOf(p);
  const [layers, setLayers] = useState<Layers | null>(null);
  const [err, setErr] = useState("");
  const [staged, setStaged] = useState<Staged>({});
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<{ write: Write; error: string }[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [spaces, setSpaces] = useState<HostedWorkspace[]>([]);
  const load = async () => {
    const [mine, theirs] = await Promise.all([
      call<EffectiveSetting[]>("GET", `/api/projects/${enc(p.id)}/settings`),
      // The owner's own values, for what Default comes to.
      call<EffectiveSetting[]>("GET", `/api/owners/${enc(owner)}/settings`).catch(() => [] as EffectiveSetting[]),
    ]);
    setLayers({ owner: theirs, projects: { [p.id]: mine } });
  };
  useEffect(() => {
    let live = true;
    load().catch((e: Error) => live && setErr(e.message));
    call<SessionSettings>("GET", `/api/projects/${enc(p.id)}/sessions/settings`).then((v) => live && setModels(v.models), () => {});
    void runs.workspaces(p.id).then((w) => live && setSpaces(w));
    return () => void (live = false);
  }, [p.id]);
  const n = changes(staged);
  useHoldLeaving(n > 0);
  if (!layers) return err ? <p class="control-error">{err}</p> : null;
  const writer = !p.team || p.team.role !== "viewer";
  const team = owner === "personal" ? null : name;
  const workspaceName = (id: string) => spaces.find((w) => w.id === id)?.name;
  const choices = (k: Key): Value[] =>
    k === "issue_home" ? ISSUE_HOMES : k === "agents_model" ? models : k === "agents_workspace" ? spaces.map((w) => w.id) : STALL_MINUTES;
  const set = (k: Key, v: Value) => setStaged(stage(staged, layers, p.id, k, v));
  const save = async () => {
    setBusy(true);
    const r = await send(writes(staged, owner, [p.id]), (w) => call("PUT", w.path, w.body));
    setFailed(r.failed);
    try {
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
    setStaged(r.left);
    setBusy(false);
  };
  const defaults = pathOf({ page: "owner-settings", owner }, names);
  return (
    <Section
      id="overrides"
      title="Overrides"
      lede={`Each setting follows ${team ? `${team}'s` : "your"} default unless this project chooses its own.`}
      more={
        <p>
          The defaults are in{" "}
          <a href={`${defaults}#defaults`} onClick={(e) => follow(e, `${defaults}#defaults`)}>
            {name}'s settings
          </a>
          , with every project's choices side by side.
        </p>
      }
      hooks={{ "data-overrides": p.id }}
    >
      {n > 0 && (
        <div class="defaults-bar" role="status" data-overrides-bar>
          <span>{n === 1 ? "1 change" : `${n} changes`}</span>
          <button class="secondary small" disabled={busy} data-overrides-cancel onClick={() => (setStaged({}), setFailed([]))}>
            Cancel
          </button>
          <button class="primary small" disabled={busy} data-overrides-save onClick={() => void save()}>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      )}
      {failed.length > 0 && (
        <p class="control-error" role="alert">
          Not saved: {failed.map((f) => `${KEYS.find((k) => k.key === f.write.key)?.name}: ${f.error}`).join("; ")}
        </p>
      )}
      <ul class="platform-list settings-rows overrides-rows">
        {KEYS.map((info) => {
          const read = settingOf(layers.projects[p.id], info.key);
          const line = overrideLine(read, info.key, team, workspaceName);
          const c = projectCell(layers, staged, p.id, info.key);
          const below = defaultOf(layers, staged, info.key);
          return (
            <li key={info.key} class={c.staged ? "defaults-staged" : undefined} data-override={info.key} data-overridden={line.overridden ? "" : undefined}>
              <b>{info.name}</b>
              <span class="dim" data-override-line>
                {c.staged ? `Not saved: ${c.overridden ? words(info.key, c.shown, workspaceName) : `back to the default, ${words(info.key, c.shown, workspaceName)}`}` : line.text}
                {writer && !c.staged && line.overridden && (
                  <>
                    {" · "}
                    <button class="link" data-override-reset={info.key} disabled={busy} onClick={() => set(info.key, null)}>
                      Reset
                    </button>
                  </>
                )}
              </span>
              {writer && (
                <select
                  class="field"
                  aria-label={info.name}
                  value={optionOf(c.own)}
                  disabled={busy}
                  data-override-select={info.key}
                  onChange={(e) => set(info.key, valueOf(info.key, e.currentTarget.value))}
                >
                  <option value="">Default ({words(info.key, below, workspaceName)})</option>
                  {choicesWith(choices(info.key), c.own).map((v) => (
                    <option key={String(v)} value={optionOf(v)}>
                      {words(info.key, v, workspaceName)}
                    </option>
                  ))}
                </select>
              )}
            </li>
          );
        })}
      </ul>
      {!writer && <p class="dim">{name}'s owners and editors choose these.</p>}
    </Section>
  );
}
