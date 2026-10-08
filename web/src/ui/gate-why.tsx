// What someone about to approve a chant gate wants to know (#617): the
// decision it enforces, the plan it binds and the member's last release.
// The daemon reads the decisions from chant's intent graph when the gate is
// raised (`graph --intent <member dir>`), so they may come a moment after
// the gate. Drawn on the workspace block's "Waiting on you", the swarm's
// card and the phone's sheet.

import type { DecisionRef, Gate } from "../proto";

/** "3m ago", "2h ago" from an RFC 3339 time. */
export function ago(t: string | null | undefined): string {
  if (!t) return "";
  const s = Math.max(0, (Date.now() - Date.parse(t)) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

/** Whether a decision governs the region chant read it for: current,
 * decided (not proposed or withdrawn), and covering it by more than a link.
 * The daemon's `DecisionRef::governs`. */
export function governs(d: DecisionRef): boolean {
  return d.current && d.relevance !== "related" && d.state !== "proposed" && d.state !== "withdrawn";
}

/** The decisions a gate enforces: those governing its member. */
export function enforced(g: Gate): DecisionRef[] | null {
  return g.why?.decisions?.filter(governs) ?? null;
}

/** The decision line, as text: what it enforces, or why that isn't known. */
export function decisionLine(g: Gate): string | null {
  const why = g.why;
  if (!why) return null;
  if (why.note) return `Decisions: ${why.note}`;
  const ds = enforced(g);
  if (!ds) return "Reading the decisions…";
  if (!ds.length) return `No decision covers ${g.member}.`;
  const more = ds.length > 1 ? ` (+${ds.length - 1} more)` : "";
  return `Enforces ${ds[0].id}${ds[0].title ? `: ${ds[0].title}` : ""}${more}`;
}

/** The plan and the last release, as text. */
export function releaseLine(g: Gate): string | null {
  const why = g.why;
  if (!why) return null;
  const parts: string[] = [];
  if (why.plan_digest) parts.push(`plan ${short(why.plan_digest)}`);
  const r = why.last_release;
  parts.push(r ? `last release ${r.component} ${ago(r.at)}${r.actor ? ` by ${r.actor}` : ""}` : "no release yet");
  return parts.join(" · ");
}

/** `sha256:` and the first 12 of a digest. */
function short(digest: string): string {
  const [algo, hex] = digest.includes(":") ? digest.split(":", 2) : ["", digest];
  return `${algo ? `${algo}:` : ""}${hex.slice(0, 12)}`;
}

/** The two lines, for a card. `compact` (the phone's sheet) draws them as one. */
export function GateWhy({ gate, compact }: { gate: Gate; compact?: boolean }) {
  const decision = decisionLine(gate);
  const release = releaseLine(gate);
  if (!decision && !release) return null;
  const ds = enforced(gate) ?? [];
  const title = ds.map((d) => `${d.id} (${d.state ?? "?"}, by ${d.relevance}): ${d.title ?? ""}`).join("\n");
  const full = gate.why?.plan_digest ?? undefined;
  if (compact) {
    return (
      <div class="dim gate-why" data-gate-why title={title}>
        {[decision, release].filter(Boolean).join(" · ")}
      </div>
    );
  }
  return (
    <div class="gate-why" data-gate-why>
      {decision && (
        <div class={ds.length ? "gate-why-decision" : "dim gate-why-decision"} data-gate-decision={ds[0]?.id ?? ""} title={title}>
          {decision}
        </div>
      )}
      {release && (
        <div class="dim gate-why-release" title={full}>
          {release}
        </div>
      )}
    </div>
  );
}
