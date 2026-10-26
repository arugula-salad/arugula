// A block whose Labs flag is off on its machine (UI audit #178): a card
// that says so and, for the owner, opens Developer settings, where the
// flags are. Before, it was "This block is in Labs" as bare text.

import { render } from "preact";
import type { Client } from "../client";
import { openDeveloper } from "../ui/developer";

export function showLabsOff(host: HTMLElement, client: Client, flag: string) {
  host.className = "block block-unknown";
  // What was there before isn't preact's ("Loading…"): cleared once, so
  // drawing the card again (on every change the client hears) keeps it.
  if (host.dataset.labsOff === undefined) host.textContent = "";
  host.dataset.labsOff = flag;
  // Flags are the owner's: anyone else just reads why.
  const owner = !client.state?.roles;
  render(
    <div class="block-labs" data-labs-off={flag}>
      <p>
        <b>This block is in Labs.</b>
      </p>
      <p class="dim">
        It shows once <code>{flag}</code> is on for this machine{owner ? "" : ": its owner turns it on"}.
      </p>
      {owner && (
        <p>
          <button class="secondary small" onClick={() => openDeveloper(client)}>
            Developer settings…
          </button>
        </p>
      )}
    </div>,
    host,
  );
}

/** Take the card down again (the flag came on). */
export function clearLabsOff(host: HTMLElement) {
  render(null, host);
  delete host.dataset.labsOff;
}
