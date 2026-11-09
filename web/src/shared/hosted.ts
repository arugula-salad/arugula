// Hosted machines (M178), the cloud workspaces' boxes: what the shell and
// the platform page call one, wherever they show it ("Control enrolls hosted machines under a project
// delegation"). Here, not in the key code, so the platform page (which
// never loads that) says the same words. `arugula_e2e::hosted::LABEL` is
// the Rust side's.

/** True of any VM: whoever runs it can read its memory. A chip's words,
 * with HOSTED_MEANS as a sentence beside it where there's room (UI audit
 * #184: "Hosted: the operator can read: it runs…" read as two colons). */
export const HOSTED_LABEL = "Hosted · operator can read";

/** What the label means, where there's room to say. */
export const HOSTED_MEANS =
  "it runs in a box Arugula's operator runs, so whoever runs it can read its memory, as with any VM. Only its project's people reach it.";

/** The direct URLs a client may try for a machine the directory lists:
 * none for a hosted machine, whatever the directory says. It's reached
 * only through control's relay. */
export function directUrls(d: { urls?: string[]; hosted?: unknown }): string[] {
  return d.hosted ? [] : (d.urls ?? []);
}
