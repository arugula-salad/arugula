// Project defaults and their overrides (Arugula #897): what an owner (a team,
// or Personal) sets for every project it has, and what a project sets for
// itself instead. Control answers each layer's settings as a list of keys,
// each with its effective value and the layer it came from
// (`GET /api/owners/{owner}/settings`, `GET /api/projects/{project}/settings`),
// and takes one key at a time (`PUT … {key, value}`); a null value clears
// that layer's own, so the layer below answers again.
//
// Changes are staged here, not sent as they're made: a cell holds what
// it'll be, Save sends one PUT per changed cell, and Cancel drops them.
// No Preact here, so the tests run it with `node --experimental-strip-types`.

/** Whose a project is: a team's id, or `personal`, and its name. An
 * older control sends no `owner`, and the team's project says. */
export function ownerOf(p: { owner?: { kind: "team"; id: string; name: string } | { kind: "personal"; account: string }; team: { id: string; name: string } | null }): { owner: string; name: string } {
  if (p.owner?.kind === "team") return { owner: p.owner.id, name: p.owner.name || "Team" };
  if (!p.owner && p.team) return { owner: p.team.id, name: p.team.name || "Team" };
  return { owner: "personal", name: "Personal" };
}

/** Where a key's effective value came from. */
export type Source = "project" | "owner" | "default";

/** A key, as control answers it. The wire type isn't in the open repo
 * yet (cloud's `crates/platform/src/settings.rs`). */
export interface EffectiveSetting {
  key: string;
  value: unknown;
  source: Source;
}

export type Key = "default_project" | "issue_home" | "agents_model" | "agents_workspace" | "stall_minutes";

/** What a cell holds: a project id, a choice, a number of minutes, or
 * null for none of the layer's own. */
export type Value = string | number | null;

export interface KeyInfo {
  key: Key;
  name: string;
  /** Whether an owner sets it for its projects. */
  owner: boolean;
  /** Whether a project sets it for itself. */
  project: boolean;
  /** What it is when no layer sets it. */
  builtin: Value;
}

/** The keys a project overrides, in the table's order. `default_project`
 * is the owner's alone, in its General section. */
export const KEYS: KeyInfo[] = [
  { key: "issue_home", name: "Where new issues go", owner: true, project: true, builtin: null },
  { key: "agents_model", name: "Agent model", owner: true, project: true, builtin: null },
  { key: "agents_workspace", name: "Agent workspace", owner: false, project: true, builtin: null },
  { key: "stall_minutes", name: "Stalled after", owner: true, project: true, builtin: 30 },
];

export const infoOf = (key: Key): KeyInfo => KEYS.find((k) => k.key === key) ?? { key, name: key, owner: true, project: false, builtin: null };

/** Where new issues go, as the project's own setting words it (../pages/issues). */
export const ISSUE_HOMES: Value[] = ["native", "github"];

/** How long before a row counts as stalled, in minutes. Board settings
 * takes any number; these are the common ones, and a cell keeps any
 * other it already has. */
export const STALL_MINUTES: Value[] = [15, 30, 60, 120, 240, 480, 1440];

/** What a value says, in a cell or a line. `name` gives a project's or a
 * workspace's name for its id. */
export function words(key: Key, v: Value, name: (id: string) => string | undefined = () => undefined): string {
  switch (key) {
    case "issue_home":
      return v === "native" ? "Here, on Arugula" : v === "github" ? "On GitHub" : "GitHub when linked";
    case "agents_model":
      return v === null ? "The strongest" : String(v).charAt(0).toUpperCase() + String(v).slice(1);
    case "agents_workspace":
      return v === null ? "None chosen" : (name(String(v)) ?? String(v));
    case "stall_minutes": {
      const n = Number(v ?? 30);
      return n >= 60 && n % 60 === 0 ? `${n / 60} hour${n === 60 ? "" : "s"}` : `${n} minutes`;
    }
    case "default_project":
      return v === null ? "None" : (name(String(v)) ?? String(v));
  }
}

/** A key's setting in a layer's answer. */
export const settingOf = (list: EffectiveSetting[] | undefined, key: Key): EffectiveSetting | undefined => list?.find((s) => s.key === key);

/** A layer's own value for a key: what it set itself, or null when a
 * layer below answers. */
export function ownOf(s: EffectiveSetting | undefined, layer: "owner" | "project"): Value {
  return s && s.source === layer ? ((s.value ?? null) as Value) : null;
}

/** What the page has read: the owner's settings, and each project's. */
export interface Layers {
  owner: EffectiveSetting[];
  projects: Record<string, EffectiveSetting[]>;
}

/** The cells changed and not yet saved: each one's new own value, by
 * `cellId`. */
export type Staged = Record<string, Value>;

/** A cell: the owner's default for a key, or a project's. */
export const OWNER = "owner";
export const cellId = (scope: string, key: Key) => `${scope}/${key}`;

/** The value a cell had when it was read. */
export function savedOf(layers: Layers, scope: string, key: Key): Value {
  return scope === OWNER ? ownOf(settingOf(layers.owner, key), "owner") : ownOf(settingOf(layers.projects[scope], key), "project");
}

/** `staged` with a cell set to `value`; set back to what was read, it's
 * no longer a change. */
export function stage(staged: Staged, layers: Layers, scope: string, key: Key, value: Value): Staged {
  const id = cellId(scope, key);
  const { [id]: _, ...rest } = staged;
  return value === savedOf(layers, scope, key) ? rest : { ...rest, [id]: value };
}

/** How many cells have changed: the bar's count. */
export const changes = (staged: Staged) => Object.keys(staged).length;

/** What a cell shows, staged changes included. `own`: the layer's own
 * value (null: it follows the layer below); `shown`: what it comes to;
 * `overridden`: it has its own; `staged`: it changed since it was read. */
export interface Cell {
  own: Value;
  shown: Value;
  overridden: boolean;
  staged: boolean;
}

/** The owner's default for a key: its own, else the built-in value. A
 * key the owner layer doesn't take shows what control said. */
export function ownerCell(layers: Layers, staged: Staged, key: Key): Cell {
  const id = cellId(OWNER, key);
  const own = id in staged ? staged[id] : savedOf(layers, OWNER, key);
  const read = settingOf(layers.owner, key);
  const builtin = read && read.source === "default" ? ((read.value ?? null) as Value) : infoOf(key).builtin;
  return { own, shown: own ?? builtin, overridden: own !== null, staged: id in staged };
}

/** What a project on the default gets: the owner's, or the built-in
 * value for a key the owner doesn't set. */
export function defaultOf(layers: Layers, staged: Staged, key: Key): Value {
  return infoOf(key).owner ? ownerCell(layers, staged, key).shown : infoOf(key).builtin;
}

/** A project's cell: its own value, else the owner's default as it
 * stands with the staged changes (the built-in value, for a key the
 * owner doesn't set). */
export function projectCell(layers: Layers, staged: Staged, project: string, key: Key): Cell {
  const id = cellId(project, key);
  const own = id in staged ? staged[id] : savedOf(layers, project, key);
  const below = defaultOf(layers, staged, key);
  return { own, shown: own ?? below, overridden: own !== null, staged: id in staged };
}

/** One PUT of a key, to send on Save. */
export interface Write {
  scope: string;
  key: Key;
  value: Value;
  path: string;
  body: { key: Key; value: Value };
}

/** The PUTs Save sends, one per changed cell: the owner's first, so a
 * project's answer reflects the new default, then each project's, in
 * the order given. */
export function writes(staged: Staged, owner: string, projects: string[]): Write[] {
  const enc = encodeURIComponent;
  const out: Write[] = [];
  for (const scope of [OWNER, ...projects]) {
    for (const { key } of [{ key: "default_project" as Key }, ...KEYS]) {
      const id = cellId(scope, key);
      if (!(id in staged)) continue;
      const value = staged[id];
      const path = scope === OWNER ? `/api/owners/${enc(owner)}/settings` : `/api/projects/${enc(scope)}/settings`;
      out.push({ scope, key, value, path, body: { key, value } });
    }
  }
  return out;
}

/** Sends `list` one at a time and keeps going past a refusal. What's
 * left staged is what failed, with why. */
export async function send(list: Write[], put: (w: Write) => Promise<unknown>): Promise<{ left: Staged; failed: { write: Write; error: string }[] }> {
  const left: Staged = {};
  const failed: { write: Write; error: string }[] = [];
  for (const w of list) {
    try {
      await put(w);
    } catch (e) {
      left[cellId(w.scope, w.key)] = w.value;
      failed.push({ write: w, error: (e as Error).message });
    }
  }
  return { left, failed };
}

/** A project's Overrides line for a key: "Overridden: …", or which
 * default it's using. `team`: the owner's name, when it's a team. */
export function overrideLine(s: EffectiveSetting | undefined, key: Key, team: string | null, name?: (id: string) => string | undefined): { overridden: boolean; text: string } {
  const v = (s?.value ?? infoOf(key).builtin) as Value;
  const said = words(key, v, name);
  if (s?.source === "project") return { overridden: true, text: `Overridden: ${said}` };
  if (s?.source === "owner") return { overridden: false, text: `${team ? "Using team default" : "Using your default"}: ${said}` };
  return { overridden: false, text: `Using the default: ${said}` };
}

/** A select's choices: the list, with the value a cell has if the list
 * doesn't. */
export function choicesWith(list: Value[], v: Value): Value[] {
  return v === null || list.includes(v) ? list : [...list, v];
}

/** A select's value as a string, and back: "" is null, and a number
 * stays one. */
export const optionOf = (v: Value) => (v === null ? "" : String(v));
export function valueOf(key: Key, s: string): Value {
  if (s === "") return null;
  return key === "stall_minutes" ? Number(s) : s;
}
