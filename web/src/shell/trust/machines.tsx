// Adding a machine (#98): the no-machines screen with the account's menu
// (#97) and any team it's waiting on (#103), the install and join steps, a
// team's join command (#332), a hosted VM (M20), and looking up a join
// code.

import { useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import { cloudPanes, useCloudPanes } from "../../shared/cloud";
import { CopyText } from "../../shared/ui/copy";
import { useSubscribe } from "../../shared/ui/hooks";
import { MenuLayer } from "../../shared/ui/menu";
import type { ControlSession, Team } from "../control";
import { splitJoinCode } from "../e2e/cert.ts";
import { AccountButton } from "../ui/app-bar";
import { openPanel, useControl } from "./common";

/** Ready, but no machine to show yet: the account's menu (#97), any team
 * this account is waiting on (#103), and how to add a machine (#98). */
export function NoMachines({ s, newAgent }: { s: ControlSession; newAgent?: () => void }) {
  useCloudPanes();
  useSubscribe((fn) => chat.subscribe(fn));
  return (
    <div class="control-page">
      <header class="control-account" data-account-bar>
        <span class="control-who">Arugula</span>
        <AccountButton client={null} />
      </header>
      <MenuLayer />
      <main>
        {s.asked.map((a) => (
          <p key={a.team} class="control-asked" data-asked={a.team}>
            Waiting for {a.owners.length ? a.owners.join(" or ") : "an owner"} to add you to <b>{a.name}</b>. Their machines appear here when they do.
          </p>
        ))}
        {/* M123: chat is control's, and needs no machine. */}
        {chat.on && (
          <button data-open-chat onClick={() => (location.hash = "chat")}>
            Chat
          </button>
        )}
        {cloudPanes.enabled && newAgent && <button class="primary" onClick={newAgent}>New agent</button>}
        <button onClick={openPanel("cloud")}>Cloud agents</button>
        {cloudPanes.enabled && <button data-open-cloud-workspaces onClick={() => (location.hash = "cloud-ws")}>Cloud workspaces</button>}
        <h1>{s.asked.length ? "Add your own machine" : "Add a machine"}</h1>
        <AddMachine s={s} />
      </main>
    </div>
  );
}

const INSTALL = "curl -fsSL https://arugula.io/install.sh | sh";
const INSTALL_WINDOWS = "irm https://arugula.io/install.ps1 | iex";

export function AddMachine({ s }: { s: ControlSession }) {
  return (
    <div class="control-add">
      <ol class="control-steps">
        <li>
          <b>Install Arugula on the machine.</b> On macOS or Linux:
          <CopyText text={INSTALL} data-install />
          On Windows, in PowerShell:
          <CopyText text={INSTALL_WINDOWS} data-install-windows />
          <span class="dim">
            Or with <a href="https://arugula.io/#install" target="_blank" rel="noopener">Homebrew, or from source</a>.
          </span>
        </li>
        <li>
          <b>Join it to this account:</b>
          <CopyText text={`~/.local/bin/arugulad join ${s.info.url}`} data-join-cmd />
          On Windows, in a new PowerShell window:
          <CopyText text={`arugulad join ${s.info.url}`} data-join-cmd-windows />
          {s.teams
            .filter((t) => !t.locked || t.role === "owner")
            .map((t) => (
              <TeamJoin key={t.team} s={s} t={t} />
            ))}
        </li>
        <li>
          <b>Approve it here.</b> It prints a link and a code: open the link here, or type the code below. Codes last 15 minutes.
          <JoinCodeForm s={s} />
        </li>
      </ol>
      <p class="dim">
        A <i>machine</i> runs your terminals; a <i>device</i> (this browser, your phone) reaches them.
      </p>
      {s.sandboxesOpen ? <HostedVm s={s} /> : null}
    </div>
  );
}

/** How a machine joins a team (#332: any member adds their own), in the
 * same words on Add a machine and in Teams. */
export function TeamJoin({ s, t, here }: { s: ControlSession; t: Team; here?: boolean }) {
  return (
    <>
      <span class="dim">
        To add a machine to <b>{t.roster.name}</b>, run this on it, or move one you have with <i>Team …</i> in <i>Devices and machines…</i>:
      </span>
      <CopyText text={`arugulad join ${s.info.url} --team ${t.team}`} {...(here ? { "data-team-join": true } : { "data-team-join-step": t.team })} />
    </>
  );
}

/** A hosted VM (M20): no machine of your own needed. */
function HostedVm({ s }: { s: ControlSession }) {
  useControl(s);
  const [err, setErr] = useState("");
  const starting = s.starting ? s.sandboxes.find((x) => x.id === s.starting) : undefined;
  return (
    <div class="hosted-vm">
      <p>No machine to hand? Use a hosted VM: a fresh Linux machine, deleted when you close its last tab.</p>
      <button class="primary" data-start-vm disabled={!!starting && !starting.state.startsWith("failed")} onClick={() => s.startSandbox().catch((e: Error) => setErr(e.message))}>
        {starting && !starting.state.startsWith("failed") ? `Starting (${starting.state})…` : "Start a hosted VM"}
      </button>
      {starting?.state.startsWith("failed") ? <p class="control-error">{starting.state}</p> : null}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

function JoinCodeForm({ s }: { s: ControlSession }) {
  const [code, setCode] = useState("");
  return (
    <form
      class="control-code"
      onSubmit={(e) => {
        e.preventDefault();
        // The whole code, secret too (#760): it stays in this page.
        const j = splitJoinCode(code.trim());
        location.hash = `join=${j ? (j.secret ? `${j.code}.${j.secret}` : j.code) : code.trim()}`;
      }}
    >
      <input class="control-join-code" placeholder="XXXXX-XXXXX.XXXX-XXXX-XXXX-XXXX" value={code} onInput={(e) => setCode((e.target as HTMLInputElement).value)} aria-label="Join code" />
      <button type="submit" disabled={!code.trim() || s.phase !== "ready"}>
        Look up
      </button>
    </form>
  );
}
