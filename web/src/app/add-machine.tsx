// Add a machine (Lift 7, #1487): Machines' one screen for it, from its
// header and from a project's settings' old Machines address. It asks
// control for a join token (`POST /api/join-tokens`, aru-141, Arugula
// #1152), for the project's team, or the reader's own account on a
// personal project, and gives the line to run on the machine, which joins
// at once with no browser. The token is good once, until it runs out, and
// shown only here. A team's viewers get control's refusal: its owners and
// editors make them. The CLI makes the same with `arugula join-token`.

import { useState } from "preact/hooks";
import { Button } from "../shared/kit/button";
import { Sheet } from "../shared/kit/sheet";
import { ago, when } from "../shared/time";
import { CopyText } from "../shared/ui/copy";
import { hubCall } from "../platform/settings/teams";
import { ownerOf } from "../platform/settings/overrides";
import type { PlatformProject } from "../platform/session";

/** control-wire's `JoinTokenMade`: `expires` is in seconds. It isn't in
 * the web's generated types. */
interface JoinTokenMade {
  token: string;
  expires: number;
}

export function AddMachine({ p, onClose }: { p: PlatformProject; onClose: () => void }) {
  const { owner, name } = ownerOf(p);
  const team = owner === "personal" ? undefined : owner;
  const [made, setMade] = useState<JoinTokenMade | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const make = async () => {
    setBusy(true);
    setErr("");
    try {
      setMade(await hubCall<JoinTokenMade>("POST", "/api/join-tokens", team ? { team } : {}));
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const runsOut = made ? made.expires * 1000 : 0;
  return (
    <Sheet title={team ? `Add a machine to ${name}` : "Add a machine"} onClose={onClose}>
      <div class="add-machine" data-join-token>
        {made ? (
          <>
            <p class="m-meta">Run this on the machine. It's shown once.</p>
            <CopyText text={`arugulad join --token ${made.token}`} data-join-token-line />
            <p class="m-meta" data-join-token-expires>
              Good once, until {when(runsOut)} ({ago(runsOut)}).
            </p>
          </>
        ) : null}
        {err && (
          <p class="k-needs" role="alert" data-join-token-error>
            {err}
          </p>
        )}
        <div class="k-row-actions">
          <Button kind={made ? "plain" : "primary"} data-join-token-make disabled={busy} onClick={() => void make()}>
            {made ? "Make another" : "Make the command"}
          </Button>
          <Button onClick={onClose}>Done</Button>
        </div>
      </div>
    </Sheet>
  );
}
