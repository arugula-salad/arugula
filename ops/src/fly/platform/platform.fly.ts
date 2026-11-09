// Control's platform (arugula-platform), as packaging/platform/fly.toml
// declares it and `fly deploy` left its two Machines (read 2026-11-02): the
// plan `fly-release` applies with the release's image (src/platform.component.ts).
// It releases platformA and gives platformB the same image (`images`).
//
// No image here: the release hands it over (`chant release cut --image`), so
// this plan is the same bytes from one release to the next unless a Machine
// itself changes. The nightly dump's Machine, in the same app, has a plan of
// its own (../platform-dump), since it runs another image.
//
// No public address: its one service is for Flycast (fly.toml says why).
// Its secrets are the app's, so a Machine update keeps them.
import {
  App,
  Machine,
  MachineConfig,
  MachineGuest,
  MachinePort,
  MachineRestart,
  MachineService,
  MachineServiceCheck,
  MachineServiceConcurrency,
  StopConfig,
} from "@intentius/chant-lexicon-fly";

export const platformApp = new App({ name: "arugula-platform", org_slug: "jake-gaylor" });

/** Both Machines' config: they differ only in their names. */
function serving() {
  return new MachineConfig({
    // What a Machine boots before a release puts its own image on it.
    image: "registry.fly.io/arugula-platform:declared",
    env: {
      ARUGULA_PLATFORM_LISTEN: "[::]:7700",
      ARUGULA_PLATFORM_REQUIRE_POSTGRES: "1",
      ARUGULA_PLATFORM_HUB: "http://illogical-control.internal:7691",
      ARUGULA_CONTROL_URL: "https://control.arugula.io",
      ARUGULA_PLATFORM_HOST: "app.arugula.io",
      ARUGULA_PREVIEW_DOMAIN: "arugula.site",
      // What `fly deploy` sets, kept so `fly status` and a hotfix's deploy see the same Machines.
      FLY_PROCESS_GROUP: "app",
      PRIMARY_REGION: "iad",
    },
    guest: new MachineGuest({ cpu_kind: "shared", cpus: 1, memory_mb: 512 }),
    restart: new MachineRestart({ policy: "on-failure", max_retries: 10 }),
    // On a stop, the platform hands back its Cloud jobs (up to 10 s), then flushes analytics.
    stop_config: new StopConfig({ signal: "SIGTERM", timeout: "25s" }),
    metadata: { fly_process_group: "app" },
    services: [
      new MachineService({
        protocol: "tcp",
        internal_port: 7700,
        autostop: false,
        autostart: false,
        min_machines_running: 2,
        ports: [new MachinePort({ port: 7700 })],
        concurrency: new MachineServiceConcurrency({ type: "connections", soft_limit: 4000, hard_limit: 5000 }),
        checks: [
          new MachineServiceCheck({
            type: "http",
            method: "get",
            protocol: "http",
            path: "/platform.json",
            interval: "15s",
            timeout: "5s",
            grace_period: "10s",
          }),
        ],
      }),
    ],
  });
}

export const platformA = new Machine({ name: "shy-snowflake-5940", region: "iad", config: serving() });

export const platformB = new Machine({ name: "dawn-tree-5447", region: "iad", config: serving() });
