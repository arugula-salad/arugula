// J1 on a real Mac (#551): web/e2e/journey-j1.spec.ts's path, with the
// real app from its release zip in a fresh tart VM (testnet/macos/vm.sh),
// its own launch agent and daemon, and the real Safari it opens links in.
// Nothing is emulated but GitHub. Every step acts the way a person does,
// through what the screen shows: System Events presses buttons and links
// by their visible names in the app's window and in Safari, and reads
// their text. Only the terminal's text is read another way (`arugula
// capture` in the VM: the canvas has none).
//
// Control and the fake GitHub run here, reached from the VM through ssh
// tunnels at the same loopback addresses. The daemon gets this control as
// ARUGULA_CONTROL in the app's launch agent (the bundle signed again, ad
// hoc as released), and the run stops before Connect if the app would go
// anywhere else: it never touches the hosted control.
//
// The report is J1-mac in web/journey-reports/ (local, never committed):
// the same graph as J1's, with the Mac's screen at each step.
//
//   node --experimental-strip-types testnet/macos/journey-j1.ts
//   (or: just macos journey)
//   ARUGULA_MACOS_APP_ZIP=URL|path  another app (default: app-latest, what a new user downloads)
//   KEEP=1                          leave the VM up
//
// Exit codes: 0 the journey held (every step led by the screen, or known),
// 1 it didn't.

import { Journey, Stopped } from "../../web/e2e/journey/record.ts";
import { firstRunMac, startMac, stopMac } from "./journey-lib.ts";

let ok = false;
const j = new Journey("J1-mac", "one person, unregistered to working (the real Mac app in a fresh VM, Safari)", undefined, ["newcomer"]);
try {
  const { control, version } = await startMac("newcomer");
  console.log(`J1-mac: the app ${version}, control ${control}`);
  await firstRunMac(j, "newcomer", control);
  await j.finish();
  ok = true;
} catch (e) {
  console.error(e instanceof Stopped ? e.message : e);
  j.write();
} finally {
  stopMac();
}
process.exit(ok ? 0 : 1);
