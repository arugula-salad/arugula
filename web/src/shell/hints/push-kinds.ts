// The push kinds hint's fact: this device's push is on, through control,
// which pushes the account about chat, machines and projects, each of
// which can be turned off (the Notifications items, ui/notify).

import type { Client } from "../client";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { pushKinds, pushNow, subscribePush } from "../ui/notify";

let watching = false;

export function pushOnControl(_client: Client): Fact | null {
  if (!watching) {
    watching = true;
    subscribePush(factChanged);
  }
  return pushNow() === "on" && pushKinds() ? { args: {} } : null;
}
