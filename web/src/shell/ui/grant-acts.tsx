// A grant card's three answers (#578): Grant, Not now, Never for this room.
// Grant and Never are the owner's own calls to control, over their session
// (`grantRead`, `grantNever`); Not now only dismisses the card, and the
// mention stays for a later grant. The card goes once control took the
// answer, or stays with the reason it was refused.

import { useState } from "preact/hooks";
import { grantNever, grantRead } from "../../shared/grant-ask";
import { GRANT_HELP, grantQuestion } from "../../shared/notice";
import type { Notice } from "../../shared/proto";

export function GrantActs({ n, done, sheet }: { n: Notice; done: () => void; sheet?: boolean }) {
  const [err, setErr] = useState("");
  const answer = (act: (n: Notice) => Promise<void>) => () => {
    setErr("");
    act(n).then(done, (e: Error) => setErr(e.message));
  };
  return (
    <>
      <button class={sheet ? "sheet-act" : "primary"} data-grant-yes onClick={answer(grantRead)} title={`${grantQuestion(n)} The mention is delivered once they can.`}>
        Grant
      </button>
      <button class={sheet ? "sheet-act" : "secondary"} data-grant-later onClick={done} title={GRANT_HELP}>
        Not now
      </button>
      <button class={sheet ? "sheet-act" : "secondary"} data-grant-never onClick={answer(grantNever)} title={GRANT_HELP}>
        Never for this room
      </button>
      {err && (
        <span class="control-error" role="alert">
          {err}
        </span>
      )}
    </>
  );
}
