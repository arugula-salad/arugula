// The question about usage counts (the telemetry track): a card in the
// corner, not a dialog, so nothing waits on it. Control's page asks for the
// account (M104, ./account-telemetry.tsx); the machine's web client
// asks for the machine (M103, shell/ui/machine-telemetry.tsx), with its own
// words and the machine's preview under *What's sent*. Closing it without
// answering asks again on the next load.

export function TelemetryCard({
  about,
  preview,
  answer,
  later,
}: {
  /** What would be sent, and what never is. */
  about: string;
  /** The next report as it would go (`telemetry.get`'s preview), shown
   * under *What's sent*. */
  preview?: unknown;
  answer: (on: boolean) => void;
  later: () => void;
}) {
  return (
    <div class="telemetry-card" role="dialog" aria-labelledby="telemetry-card-title" data-telemetry-card>
      <button class="modal-close" aria-label="Not now" title="Not now" onClick={later}>
        ×
      </button>
      <h2 id="telemetry-card-title">Help improve Arugula?</h2>
      <p>{about}</p>
      {preview !== undefined && <TelemetryPreview preview={preview} />}
      <div class="prompt-buttons">
        <button class="secondary" data-telemetry-no onClick={() => answer(false)}>
          No thanks
        </button>
        <button class="primary" data-telemetry-yes onClick={() => answer(true)}>
          Yes, send them
        </button>
      </div>
    </div>
  );
}

/** *What's sent*: the report as JSON, folded away until asked for. */
export function TelemetryPreview({ preview }: { preview: unknown }) {
  return (
    <details class="telemetry-preview" data-telemetry-preview>
      <summary>What's sent</summary>
      <pre>{JSON.stringify(preview, null, 2)}</pre>
    </details>
  );
}
