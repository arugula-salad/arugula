// Machines, a section of a project's settings (Arugula #1152): Make a join
// token asks control for one (`POST /api/join-tokens`, aru-141), for the
// project's team, or for the reader's own account on a personal project.
// A machine joins with it at once, `arugulad join --token ajt_…`, with no
// browser. It's good once, until it runs out, and shown only here, until
// the page is left. A team's viewers get control's refusal: its owners and
// editors make them. The CLI makes the same with `arugula join-token`.

import { useState } from "preact/hooks";
import { ago, when } from "../../shared/time";
import { CopyText } from "../../shared/ui/copy";
import { Section } from "../../shared/ui/settings-section";
import { hubCall } from "./teams";
import { ownerOf } from "./overrides";
import type { PlatformProject } from "../session";

/** control-wire's `JoinTokenMade`: `expires` is in seconds. It isn't in
 * the web's generated types. */
interface JoinTokenMade {
  token: string;
  expires: number;
}

export function JoinToken({ p }: { p: PlatformProject }) {
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
    <Section
      id="machines"
      title="Machines"
      summary={made ? "Join token made" : team ? `Join a machine to ${name}` : "Join a machine of yours"}
      lede={team ? `Add a machine to ${name}: make a join token, and run the line it gives on the machine.` : "Add a machine of yours: make a join token, and run the line it gives on the machine."}
      more={
        <p>
          The machine joins at once, with no browser. A token is good once, until it runs out. In a script, <code>arugula join-token</code> makes one from a
          signed-in CLI{team ? <> (<code>--team {team}</code> for this team's)</> : null}.
        </p>
      }
    >
      <div class="plane-hook" data-join-token>
        <span class="plane-hook-form">
          <button class={made ? "secondary" : "primary"} data-join-token-make disabled={busy} onClick={() => void make()}>
            {made ? "Make another" : "Make a join token"}
          </button>
        </span>
        {made && (
          <>
            <span class="dim">Run this on the machine. The token is shown this once.</span>
            <CopyText text={`arugulad join --token ${made.token}`} data-join-token-line />
            <span class="dim" data-join-token-expires>
              Good once, until {when(runsOut)} ({ago(runsOut)}).
            </span>
          </>
        )}
      </div>
      {err && (
        <p class="control-error" data-join-token-error>
          {err}
        </p>
      )}
    </Section>
  );
}
