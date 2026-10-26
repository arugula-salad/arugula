// A project's environments and planning a deploy on the platform page
// (M252): what the platform's driver list says (`GET
// /api/projects/{project}/drivers`), and the bodies the page sends to make
// an environment and to plan a deploy, and a project's history of deploys
// (`GET /api/projects/{project}/deploys`). Plain functions, tested in
// scripts/environments.test.mjs.

/** One of a driver's settings, as `arugula_e2e::drivers::setting_fields`
 * lists it. */
export interface SettingField {
  name: string;
  required: boolean;
  default: string | null;
  /** The only values it takes, when it's a choice. */
  choices: string[];
  about: string;
}

/** A secret a driver takes: `always` minted by the platform for each run
 * (an environment never holds it), minted `unless_held` by the
 * environment, or held (`null`). */
export interface DriverSecret {
  name: string;
  minted: "always" | "unless_held" | null;
}

export interface Driver {
  name: string;
  settings: SettingField[];
  secrets: DriverSecret[];
  rolls_back: boolean;
}

export interface Environment {
  id: string;
  project: string;
  name: string;
  driver: string;
  secret_names: string[];
  settings: Record<string, string>;
}

/** The secrets an environment of `d` may hold: not the ones the platform
 * always mints. */
export function holdable(d: Driver): DriverSecret[] {
  return d.secrets.filter((s) => s.minted !== "always");
}

/** What the page says of a secret the driver takes. */
export function secretNote(s: DriverSecret): string {
  if (s.minted === "always") return "minted by the platform for each run";
  if (s.minted === "unless_held") return "minted for each run unless the environment holds its own";
  return "";
}

/** An environment's name, as the platform takes it: a-z, 0-9 and `-`, up
 * to 40, not `-` first. */
export const ENV_NAME = /^[a-z0-9][a-z0-9-]{0,39}$/;

/** `POST /api/projects/{project}/environments`' body from the form, or
 * why not. Settings left empty aren't sent (the platform fills in their
 * defaults); a required one must be given. */
export function environmentBody(
  name: string,
  d: Driver | undefined,
  values: Record<string, string>,
  secrets: string[],
): { body: { name: string; driver: string; secret_names: string[]; settings: Record<string, string> } } | { error: string } {
  const n = name.trim();
  if (!ENV_NAME.test(n)) return { error: "A name: a-z, 0-9 and -, up to 40 (production, staging)." };
  if (!d) return { error: "Pick a driver." };
  const settings: Record<string, string> = {};
  for (const f of d.settings) {
    const v = (values[f.name] ?? "").trim();
    if (!v) {
      if (f.required) return { error: `${f.name} is needed: ${f.about}` };
      continue;
    }
    if (f.choices.length && !f.choices.includes(v)) return { error: `${f.name} is ${f.choices.join(" or ")}.` };
    settings[f.name] = v;
  }
  const may = holdable(d).map((s) => s.name);
  const secret_names = secrets.filter((s) => may.includes(s));
  return { body: { name: n, driver: d.name, secret_names, settings } };
}

/** A full commit id: 40 hex. */
export const COMMIT = /^[0-9a-f]{40}$/;

/** `POST /api/projects/{project}/deploys`' body from the form, or why
 * not. */
export function planBody(
  environment: string,
  commit: string,
  repo: string,
  rollBack: boolean,
  repoOk: RegExp,
): { body: { environment: string; source: { kind: "commit"; ref: string; where: string }; roll_back: boolean } } | { error: string } {
  if (!environment) return { error: "Pick an environment." };
  const c = commit.trim().toLowerCase();
  if (!COMMIT.test(c)) return { error: "The commit's full id: 40 hex characters (git rev-parse HEAD)." };
  const r = repo.trim();
  if (!repoOk.test(r)) return { error: "The repo on GitHub: OWNER/NAME." };
  return { body: { environment, source: { kind: "commit", ref: c, where: r }, roll_back: rollBack } };
}

/** The repo "Deploy…" offers for `env` (UI audit #147): the one its last
 * deploy came from, else the linked repo named as the environment is
 * (`beds-smoke` for `arugula-salad/beds-smoke`), else the first linked. */
export function repoFor(env: string | undefined, linked: string[], lastFrom?: string | null): string {
  if (lastFrom) return lastFrom;
  const named = env ? linked.find((r) => r.split("/")[1]?.toLowerCase() === env.toLowerCase()) : undefined;
  return named ?? linked[0] ?? "";
}

/** A deploy as a project's history lists it (`GET
 * /api/projects/{project}/deploys`): its own page has the rest. */
export interface DeploySummary {
  id: string;
  environment: { id: string; name: string; driver: string };
  source: { kind: string; ref: string; where: string };
  /** A roll back's deploy and what it goes back to, or null. */
  roll_back: { deploy: string } | null;
  state: string;
  planned_by: string;
  planned_by_name: string;
  agent: { machine: string; machine_name: string } | null;
  created_at: number;
  planned_at: number | null;
  /** When it was approved or denied. */
  decided_at: number | null;
  /** When it ended: applied, failed, denied, canceled, expired or stale. */
  finished_at: number | null;
}

/** A page of a project's history; `next` is the `before` of the page
 * after it, while there may be one. */
export interface DeployHistory {
  deploys: DeploySummary[];
  next: string | null;
}

/** The history's path: `environment`'s deploys only (its id or name),
 * at most `limit`, those after `before`. */
export function historyPath(project: string, environment?: string, limit?: number, before?: string | null): string {
  const q = new URLSearchParams();
  if (environment) q.set("environment", environment);
  if (limit) q.set("limit", String(limit));
  if (before) q.set("before", before);
  const s = q.toString();
  return `/api/projects/${encodeURIComponent(project)}/deploys${s ? `?${s}` : ""}`;
}

/** When a deploy last moved: it ended, it was decided, or it was planned. */
export const lastMoved = (d: Pick<DeploySummary, "finished_at" | "decided_at" | "created_at">) => d.finished_at ?? d.decided_at ?? d.created_at;

/** A commit's or an artifact's reference, short (../shared/deploys, which
 * control's deploy prompt shares). */
export { shortRef } from "../shared/deploys";
