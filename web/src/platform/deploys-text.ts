// A project's environments and planning a deploy on the platform page
// (M252): what the platform's driver list says (`GET
// /api/projects/{project}/drivers`), and the bodies the page sends to make
// an environment and to plan a deploy. Plain functions, tested in
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
