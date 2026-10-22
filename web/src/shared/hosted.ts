// Hosted machines (M178): what the shell and the platform page call one,
// wherever they show it ("Control enrolls hosted machines under a project
// delegation"). Here, not in the key code, so the platform page (which
// never loads that) says the same words. `arugula_e2e::hosted::LABEL` is
// the Rust side's.

/** True of any VM: whoever runs it can read its memory. */
export const HOSTED_LABEL = "Hosted: the operator can read";

/** What the label means, where there's room to say. */
export const HOSTED_MEANS =
  "it runs in a box Arugula's operator runs, so whoever runs it can read its memory, as with any VM. Only its project's people reach it.";
