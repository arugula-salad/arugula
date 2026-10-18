// The question about usage counts (the telemetry track): a card in the
// corner, not a dialog, so nothing waits on it. Control's page asks for the
// account (M104); the machine's web client asks for the machine (M103),
// with its own `whose` words. Closing it without answering asks again on
// the next load.

export function TelemetryCard({ whose, answer, later }: { whose: string; answer: (on: boolean) => void; later: () => void }) {
  return (
    <div class="telemetry-card" role="dialog" aria-labelledby="telemetry-card-title" data-telemetry-card>
      <button class="modal-close" aria-label="Not now" title="Not now" onClick={later}>
        ×
      </button>
      <h2 id="telemetry-card-title">Help improve Arugula?</h2>
      <p>
        Send usage counts {whose}: which views you open and how often, and errors our code hits. Never what your terminals show,
        names, file names, commands or prompts.
      </p>
      <div class="prompt-buttons">
        <button data-telemetry-no onClick={() => answer(false)}>
          No thanks
        </button>
        <button class="primary" data-telemetry-yes onClick={() => answer(true)}>
          Yes, send them
        </button>
      </div>
    </div>
  );
}
