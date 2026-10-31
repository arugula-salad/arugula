// What agents may write to a Plane project, per kind (Arugula #423), and what
// a write's footer says. The presets mirror `PRESETS` in the platform's
// `crates/platform/src/trackers/policy.rs`; a stored policy that matches none
// is "Custom". The platform stores per kind, never a preset's name.

import type { TrackerWritePolicy } from "../../shared/proto.gen";

export type Kind = keyof TrackerWritePolicy;

/** The kinds, with their words in the settings. */
export const KINDS: { kind: Kind; label: string }[] = [
  { kind: "create", label: "Creates" },
  { kind: "comment", label: "Comments" },
  { kind: "edit", label: "Edits (title or body)" },
  { kind: "state", label: "State changes (close, reopen)" },
  { kind: "fields", label: "Field changes (module, parent, type, labels)" },
];

const all = (v: "allow" | "approve"): TrackerWritePolicy => ({ create: v, comment: v, edit: v, state: v, fields: v });

export const PRESETS: { name: string; policy: TrackerWritePolicy }[] = [
  { name: "Approve everything", policy: all("approve") },
  { name: "Allow comments and edits, approve creates and closes", policy: { create: "approve", comment: "allow", edit: "allow", state: "approve", fields: "allow" } },
  { name: "Allow everything", policy: all("allow") },
];

export const CUSTOM = "Custom";

/** The preset a policy is, or `Custom`. */
export function presetOf(p: TrackerWritePolicy): string {
  const same = (a: TrackerWritePolicy, b: TrackerWritePolicy) => KINDS.every(({ kind }) => a[kind] === b[kind]);
  return PRESETS.find((x) => same(x.policy, p))?.name ?? CUSTOM;
}

/** The footer each attribution sends, as the help line shows it. */
export const ATTRIBUTIONS: { value: string; label: string; sends: string }[] = [
  { value: "full", label: "Full", sends: "— via Arugula, by Ada's agent on geek" },
  { value: "short", label: "Short", sends: "— via Arugula" },
  { value: "none", label: "None", sends: "nothing is added" },
];

/** One line saying what an attribution sends. */
export function attributionHelp(value: string): string {
  const a = ATTRIBUTIONS.find((x) => x.value === value);
  if (!a) return "";
  return value === "none" ? "Sends the text as written, with no line added." : `Ends each body and comment with: ${a.sends}`;
}
