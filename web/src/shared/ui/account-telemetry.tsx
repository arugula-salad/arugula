// The account's answer about usage counts from control's page (M104): the
// card that asks once, and the switch in Sign-in and account that changes
// the answer. Both take the answer and a way to give one, so neither sees
// the control session (M149). Shared (M155): the shell asks and switches
// it on control's page, through the hub, and the platform page may too.

import { useState } from "preact/hooks";
import { TelemetryCard } from "./telemetry-card";

/** The account's answer (control-wire's `telemetry::PageConsent`, which
 * the shell's `ControlSession["telemetry"]` is): `on` is null until asked,
 * and `ask` says whether this control forwards reports at all. */
export interface Consent {
  on: boolean | null;
  ask: boolean;
}

/** M104: asked once, when nothing else is (the overlay says when). */
export function TelemetryAsk({ answer, later }: { answer: (on: boolean) => void; later: () => void }) {
  return (
    <TelemetryCard
      about="Send usage counts from your account on this page: which views you open and how often, and errors our code hits. Never what your terminals show, names, file names, commands or prompts."
      answer={answer}
      later={later}
    />
  );
}

/** M104: the account's answer about usage counts from this page, the
 * card's yes or no, changed here. Not on a control that forwards none. */
export function UsageReports({ telemetry: t, setTelemetry }: { telemetry: Consent | null; setTelemetry: (on: boolean) => Promise<unknown> }) {
  const [err, setErr] = useState("");
  if (!t || (!t.ask && !t.on)) return null;
  return (
    <>
      <h3 class="control-group">Usage reports</h3>
      <p class="dim">
        Counts from your account on this page: which views you open and how often, and errors our code hits. Never what your
        terminals show, names, file names, commands or prompts.
      </p>
      <label class="control-check">
        <input
          type="checkbox"
          role="switch"
          data-telemetry-switch
          checked={!!t.on}
          onChange={(e) => {
            setErr("");
            setTelemetry((e.target as HTMLInputElement).checked).catch((x: Error) => setErr(x.message));
          }}
        />{" "}
        Usage reports from this page
      </label>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
