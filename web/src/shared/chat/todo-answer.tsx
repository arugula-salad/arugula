// A to-do's answer (Arugula #1468), where it's answered outside its Needs
// you row: its card in the staff room and an ended session's panel. The
// note, marked no secrets and checked as it's typed as the row's is (one
// that looks like a secret says so, and holds the answers back), then Done
// and Can't. `answer` sends it (`POST /api/inbox/{prp}`) and throws
// control's refusal, said here in its words; the place then draws the
// receipt from the to-do's record.

import { useState } from "preact/hooks";
import { Button } from "../kit/button";
import { TextField } from "../kit/field";
import { looksSecret } from "../secret-like";

export function TodoAnswer({ options, answer, ...data }: { options: string[]; answer: (choice: string, note: string) => Promise<void> } & { [d: `data-${string}`]: unknown }) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const secret = looksSecret(note);
  const send = async (choice: string) => {
    if (busy || secret) return;
    setBusy(true);
    try {
      await answer(choice, note);
      setSaid(null);
    } catch (e) {
      setSaid(`Couldn't answer it: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div {...data}>
      <TextField label="Note" hint="no secrets" warn={secret ? `That looks like ${secret}: leave it out` : undefined} value={note} onInput={setNote} onEnter={() => void send(options[0]!)} />
      <div class="k-row-actions">
        {options.map((o, n) => (
          <Button key={o} kind={n === 0 ? "primary" : "plain"} disabled={busy || !!secret} data-todo-answer={o} onClick={() => void send(o)}>
            {o}
          </Button>
        ))}
      </div>
      {said && (
        <p class="dim" role="status" data-todo-said>
          {said}
        </p>
      )}
    </div>
  );
}
