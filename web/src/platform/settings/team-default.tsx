// What a project's setting says under its label, where its owner (a team,
// or Personal) has a default for it (Arugula #1323, which took the place
// of the project's Overrides section): "Team default · follows Acme", or
// "Set here · team default is Opus · Reset". Reset clears the project's
// own value (`PUT /api/projects/{project}/settings {key, value: null}`), so
// the owner's default answers again. The full table of every project's
// stays in the owner's Project defaults (../pages/owner-settings).
//
// The project's and its owner's settings are read once for all of a page's
// lines (`layersOf`), and read again when a setting changes (`reread`).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { names } from "../links";
import { follow, pathOf } from "../router";
import type { PlatformProject } from "../session";
import { ownerOf, teamLine, type EffectiveSetting, type Key, type Layers } from "./overrides";

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

/** Each project's layers as last read, and who's showing them. */
const read = new Map<string, { layers: Layers | null; asked: Promise<void> | null; owner: string; shown: Set<() => void> }>();

function entry(p: PlatformProject) {
  let e = read.get(p.id);
  if (!e) read.set(p.id, (e = { layers: null, asked: null, owner: ownerOf(p).owner, shown: new Set() }));
  e.owner = ownerOf(p).owner;
  return e;
}

/** Reads a project's settings and its owner's again, and tells its lines. */
export function reread(project: string): Promise<void> {
  const e = read.get(project);
  if (!e) return Promise.resolve();
  e.asked = Promise.all([
    call<EffectiveSetting[]>("GET", `/api/projects/${enc(project)}/settings`),
    // The owner's own values, for what the default is.
    call<EffectiveSetting[]>("GET", `/api/owners/${enc(e.owner)}/settings`).catch(() => [] as EffectiveSetting[]),
  ]).then(
    ([mine, theirs]) => {
      e.layers = { owner: theirs, projects: { [project]: mine } };
      e.shown.forEach((f) => f());
    },
    () => {},
  );
  return e.asked;
}

/** A project's layers, read once for all its lines; null until they are,
 * or when they can't be. */
function useLayers(p: PlatformProject): Layers | null {
  const e = entry(p);
  const [, tick] = useState(0);
  useEffect(() => {
    const f = () => tick((n) => n + 1);
    e.shown.add(f);
    if (!e.asked) void reread(p.id);
    return () => void e.shown.delete(f);
  }, [p.id]);
  return e.layers;
}

/** The line under a setting's label. `reset`: what the setting does once
 * Reset has cleared the project's own value (read itself again). `name`
 * gives a value's words where they're a name. */
export function TeamDefault({ p, k, reset, name }: { p: PlatformProject; k: Key; reset?: () => void; name?: (id: string) => string | undefined }) {
  const layers = useLayers(p);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  if (!layers) return null;
  const { owner, name: ownerName } = ownerOf(p);
  const team = owner === "personal" ? null : ownerName;
  const line = teamLine(layers, p.id, k, team, name);
  if (!line) return null;
  const writer = !p.team || p.team.role !== "viewer";
  const defaults = pathOf({ page: "settings", scope: "team", owner, section: "defaults" }, names);
  const clear = async () => {
    setBusy(true);
    setErr("");
    try {
      await call("PUT", `/api/projects/${enc(p.id)}/settings`, { key: k, value: null });
      await reread(p.id);
      reset?.();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <p class="dim settings-team-default" data-team-default={k} data-set-here={line.own ? "" : undefined}>
      {line.own ? (
        line.text
      ) : (
        // The owner's name links to its Project defaults, where every project's are side by side.
        <>
          {line.text.slice(0, line.text.lastIndexOf(team ?? "your defaults"))}
          <a href={defaults} data-team-default-owner onClick={(e) => follow(e, defaults)}>
            {team ?? "your defaults"}
          </a>
        </>
      )}
      {line.own && writer && (
        <>
          {" · "}
          <button class="link" data-team-default-reset={k} disabled={busy} onClick={() => void clear()}>
            Reset
          </button>
        </>
      )}
      {err && (
        <span class="control-error" role="alert">
          {" "}
          {err}
        </span>
      )}
    </p>
  );
}
