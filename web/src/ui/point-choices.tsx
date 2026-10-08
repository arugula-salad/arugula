// A workspace decision point's open question (#621), answered with one of
// its choices: `chant workspace points answer` on the daemon, as the person
// who picks, so the answer is a points record chant reads. It rides a gate
// (`source.kind === "point"`), so it's drawn where gates are: the workspace
// block's "Waiting on you", the swarm's card and the phone's sheet.

import type { Gate, GateSource, PointChoice } from "../proto";

export type PointSource = Extract<GateSource, { kind: "point" }>;

/** The decision point a gate is, if it is one. */
export function pointOf(g: Gate | null | undefined): PointSource | null {
  return g?.source.kind === "point" ? g.source : null;
}

/** "A model proposes medium", when one did. */
export function proposalLine(p: PointSource): string | null {
  if (p.proposed === undefined) return null;
  const c = p.choices.find((x) => x.value === p.proposed);
  return `A model proposes ${c?.label ?? p.proposed}`;
}

/** What a choice means, for its button's tooltip. */
function tip(p: PointSource, c: PointChoice): string | undefined {
  const parts = [c.means, c.value === p.proposed ? "a model proposed this" : null].filter(Boolean);
  return parts.length ? parts.join(" · ") : undefined;
}

/** A button for each choice, the one a model proposed first among equals.
 * With none (the points read didn't say), nothing: it's answered where
 * chant runs. */
export function PointChoices({ point, answer, busy, cls }: { point: PointSource; answer: (value: string) => void; busy?: boolean; cls?: string }) {
  return (
    <>
      {point.choices.map((c) => (
        <button
          key={c.value}
          class={[cls, c.value === point.proposed ? "pri" : ""].filter(Boolean).join(" ") || undefined}
          data-answer-point={c.value}
          title={tip(point, c)}
          disabled={busy}
          onClick={() => answer(c.value)}
        >
          {c.label.charAt(0).toUpperCase() + c.label.slice(1)}
        </button>
      ))}
    </>
  );
}
