// Labs flags control keeps (#347), for an account's machines or a team's:
// the list control's Teams panel draws (shell/trust/teams.tsx) and the
// app's Settings › Team and billing › Labs (Arugula #1118) both draw, each
// flag unset, on or off. `GET /api/flags` answers [`LabsFlags`];
// `POST /api/flags` sets one.

import { FLAGS } from "../proto.gen";

/** `GET /api/flags` on control (#347). */
export type LabsFlags = {
  account: Record<string, boolean>;
  teams: Record<string, { flags: Record<string, boolean>; owner: boolean }>;
  /** A machine of the account has Developer settings: only then does the page offer these (strangers never see Labs). */
  dev?: boolean;
};

/** The flags control may set: guest ssh and the agent catalog are each
 * machine's alone (`local_only`), set in its own Developer settings. */
export const PUSHED = FLAGS.filter((f) => !f.local_only);

/** Labs flags set on control (#347), for an account's machines or a team's:
 * each unset (a machine follows its default), on or off. A machine's own
 * Developer settings win over these, and the account's over the team's. */
export function LabsFlagList({
  scope,
  what,
  flags,
  edit,
  set,
  open,
}: {
  scope: string;
  what: string;
  flags: Record<string, boolean>;
  edit: boolean;
  set: (name: string, on: boolean | null) => void;
  /** Drawn open, where it's a section's whole content. */
  open?: boolean;
}) {
  const named = PUSHED.filter((f) => flags[f.name] !== undefined).length;
  return (
    <details class="labs-flags" data-labs-flags={scope} open={open}>
      <summary>
        Developer settings <span class="dim">· {named ? `${named} set` : "none set"}</span>
      </summary>
      <p class="dim">{what}</p>
      <ul class="control-devices">
        {PUSHED.map((f) => {
          const v = flags[f.name];
          const value = v === undefined ? "" : v ? "on" : "off";
          return (
            <li key={f.name} data-labs-flag={f.name}>
              <span>
                {f.title}
                <span class="control-device-label dim">{f.about}</span>
              </span>
              {edit ? (
                <select
                  class="field"
                  value={value}
                  aria-label={`${f.title} for ${scope}`}
                  onChange={(e) => {
                    const x = (e.target as HTMLSelectElement).value;
                    set(f.name, x === "" ? null : x === "on");
                  }}
                >
                  <option value="">Not set</option>
                  <option value="on">On</option>
                  <option value="off">Off</option>
                </select>
              ) : (
                <span class="dim">{v === undefined ? "Not set" : v ? "On" : "Off"}</span>
              )}
            </li>
          );
        })}
      </ul>
    </details>
  );
}
