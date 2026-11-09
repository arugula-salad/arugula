// The platform's nightly dump (M145, docs/control-ops.md "The nightly
// dump"): a scheduled Machine in arugula-platform, outside the app's process
// groups, started by Fly every hour. dump.sh dumps once the newest dump is
// 20 h old, so about once a day. As `fly machine update` left it (read
// 2026-11-02).
//
// A release updates it with `skip_launch` and leaves it stopped
// (INTENTIUS/chant#4696). Each update restarts Fly's schedule clock, which
// is why it's hourly. No image here: the release hands over
// packaging/platform-dump's around the release's arugula-platform.
import { App, DNSConfig, Machine, MachineConfig, MachineGuest, MachineRestart } from "@intentius/chant-lexicon-fly";

export const dumpApp = new App({ name: "arugula-platform", org_slug: "jake-gaylor" });

export const platformDump = new Machine({
  name: "platform-dump",
  region: "iad",
  config: new MachineConfig({
    image: "registry.fly.io/arugula-platform:dump-declared",
    schedule: "hourly",
    env: {
      // The age public keys the dump is encrypted to (not a secret: no key on
      // Fly can open a dump). Its private half is in Infisical.
      ARUGULA_DUMP_RECIPIENTS: "age1gulmpg3ynhp0e97kam8mjultnf4efcusg987whq002yx2svg7a2ssvdwgv",
    },
    guest: new MachineGuest({ cpu_kind: "shared", cpus: 1, memory_mb: 512 }),
    // A failed run waits for the next hour.
    restart: new MachineRestart({ policy: "no" }),
    // arugula-platform.internal never names it.
    dns: new DNSConfig({ skip_registration: true }),
    // Outside the app's process groups: fly deploy and fly scale count only fly_process_group == "app".
    metadata: { role: "platform-dump" },
  }),
});
