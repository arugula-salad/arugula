// `/inbox` (and `/inbox?all=1`, an address kept from when it was scoped):
// the Inbox, every project's (Lift 6d, #1486), drawn by the app where it
// reaches machines (app/inbox.tsx, through `PlatformApi.Inbox`), so a
// pane's ask answers in place.

import { usePlatformApi } from "../../shared/platform-api";

export function InboxPage() {
  const { Inbox } = usePlatformApi();
  return Inbox ? <Inbox /> : <p class="dim">Loading…</p>;
}
