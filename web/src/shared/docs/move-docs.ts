// Put Docs from a person's own project into a team (Arugula #818): no UI, so
// the Docs list's selection and onboarding's pick of what goes into the team
// call the same move. One Doc at a time, in order, each with its own outcome:
//
// - A copy of a machine's Doc is Share with… (`POST …/copies`): the team Doc
//   with its sections and pictures. A 409 carries the team Doc that's already
//   there, and for the person that's a success.
// - A Doc written on the platform has no machine: it's made again in the team
//   (`POST …/docs`) from its sections. A person can't upload pictures, so
//   they stay behind, and the result says so.
//
// It never deletes or changes the original: whether the Mine copy is retired
// afterwards is the caller's call.

import type { ProjectDoc, ProjectDocView } from "../proto";
import { platformFetch, refusalOf } from "../maintenance.ts";

/** What became of one Doc. */
export interface MoveResult {
  /** The Doc in `from`. */
  id: string;
  title: string;
  /** `made` a team Doc; one was `there` already (`teamDoc`); or it was `refused` (`why`). */
  outcome: "made" | "there" | "refused";
  /** The team's Doc, for made and there. */
  teamDoc?: string;
  why?: string;
  /** Made again from a Doc written here, which has pictures that didn't come. */
  noPictures?: boolean;
}

/** What a result says, as the Docs list's bar and a single Doc's Share both
 * put it. `team` is the team's name. */
export const resultWords = (r: MoveResult, team: string) => (r.outcome === "made" ? "Made" : r.outcome === "there" ? `Already in ${team}` : `Refused: ${r.why}`);

const enc = encodeURIComponent;

/** One call as the person; `body` is `{}` when the answer isn't JSON. */
async function call(method: string, path: string, body?: unknown) {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const json = (await res.json().catch(() => ({}))) as Record<string, any>;
  return { ok: res.ok, status: res.status, json, why: refusalOf(res.status, json, `HTTP ${res.status}`) };
}

async function moveOne(from: string, team: string, d: ProjectDoc): Promise<MoveResult> {
  const result = { id: d.id, title: d.title };
  try {
    if (d.machine) {
      const r = await call("POST", `/api/projects/${enc(from)}/docs/${enc(d.id)}/copies`, { to: team });
      if (r.ok) return { ...result, outcome: "made", teamDoc: (r.json as ProjectDocView).doc.id };
      if (r.status === 409 && typeof r.json.id === "string") return { ...result, outcome: "there", teamDoc: r.json.id };
      return { ...result, outcome: "refused", why: r.why };
    }
    const got = await call("GET", `/api/projects/${enc(from)}/docs/${enc(d.id)}`);
    if (!got.ok) return { ...result, outcome: "refused", why: got.why };
    const view = got.json as ProjectDocView;
    const sections = view.sections.map((s) => ({ heading: s.heading, intent: s.intent, markdown: s.body }));
    const r = await call("POST", `/api/projects/${enc(team)}/docs`, { title: d.title, sections });
    if (!r.ok) return { ...result, outcome: "refused", why: r.why };
    const noPictures = (view.assets?.length ?? 0) > 0;
    return { ...result, outcome: "made", teamDoc: (r.json as ProjectDocView).doc.id, ...(noPictures && { noPictures }) };
  } catch (e) {
    // The platform couldn't be reached: this Doc's outcome, and the rest still go.
    return { ...result, outcome: "refused", why: (e as Error).message };
  }
}

/** Move `docs` (Docs of the project `from`) into the team project `team`.
 * `onProgress` hears each result as it lands, with how many are done of how many. */
export async function moveDocsToTeam(from: string, team: string, docs: ProjectDoc[], onProgress?: (done: MoveResult, n: number, of: number) => void): Promise<MoveResult[]> {
  const out: MoveResult[] = [];
  for (const d of docs) {
    const r = await moveOne(from, team, d);
    out.push(r);
    onProgress?.(r, out.length, docs.length);
  }
  return out;
}
