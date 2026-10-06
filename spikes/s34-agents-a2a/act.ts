// As a person's laptop in the S34 rig: list what waits on a machine, or
// answer an A2A card there.
//
//   node --experimental-strip-types --no-warnings spikes/s34-agents-a2a/act.ts STATE_DIR WHO MACHINE [list|allow|always|deny]

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Device } from "../../web/fixtures/device.ts";

const [root, who, machine, what = "list"] = process.argv.slice(2);
const rig = JSON.parse(readFileSync(join(root, "rig.json"), "utf8"));
const me = await Device.load(rig.people[who].device);
const id = rig.machines[machine].id;
const sock = await me.connect(id, undefined, 8000);
const items = await (await sock.request("GET", "/api/attention")).json();
const cards = (items as { pane: number; state: string; reason?: { headline?: string } }[]).filter((i) => i.state === "needs_input");
if (what === "list") {
  console.log(JSON.stringify(process.env.ALL ? items : cards.map((c) => ({ pane: c.pane, says: c.reason?.headline })), null, 2));
} else {
  const c = cards[0];
  if (!c) {
    console.log("nothing waits");
    process.exit(1);
  }
  const body =
    what === "deny"
      ? { action: "deny", pane: c.pane, message: "not now" }
      : { action: "allow", pane: c.pane, option: what === "always" ? "always" : "once" };
  const r = await sock.request("POST", "/api/attention/act", body);
  console.log(r.status, await r.text());
}
sock.close();
process.exit(0);
