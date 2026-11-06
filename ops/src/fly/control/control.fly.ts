// Control's hub (illogical-control), as packaging/control/fly.toml declares
// it and `fly deploy` left its one Machine (read 2026-11-02): the plan
// `fly-release` applies with the release's image (src/control.component.ts).
//
// No image here: the release hands it over (`chant release cut --image`), so
// this plan is the same bytes from one release to the next unless the
// Machine itself changes, and an unchanged hub isn't redeployed.
import {
  App,
  Machine,
  MachineConfig,
  MachineGuest,
  MachineMount,
  MachinePort,
  MachineRestart,
  MachineService,
  MachineServiceCheck,
  MachineServiceConcurrency,
  StopConfig,
} from "@intentius/chant-lexicon-fly";

export const controlApp = new App({ name: "illogical-control", org_slug: "jake-gaylor" });

export const control = new Machine({
  name: "throbbing-surf-5942",
  region: "ewr",
  config: new MachineConfig({
    // What a Machine boots before a release puts its own image on it.
    image: "registry.fly.io/illogical-control:declared",
    env: {
      ARUGULA_CONTROL_URL: "https://control.arugula.io",
      ARUGULA_CONTROL_LISTEN: "0.0.0.0:7690",
      ARUGULA_CONTROL_DB: "/data/control.db",
      ARUGULA_CONTROL_PROXY_HEADER: "Fly-Client-IP",
      ARUGULA_RELAY_MAX_TOTAL: "5000",
      ARUGULA_CONTROL_BLOCK_DOMAIN: "arugula.run",
      ARUGULA_PREVIEW_DOMAIN: "arugula.site",
      ARUGULA_CONTROL_APPLE_TEAM_ID: "HNDPMQPF22",
      // What `fly deploy` sets, kept so `fly status` and a hotfix's deploy see the same Machine.
      FLY_PROCESS_GROUP: "app",
      PRIMARY_REGION: "ewr",
    },
    guest: new MachineGuest({ cpu_kind: "shared", cpus: 1, memory_mb: 1024 }),
    // control_data (10 GB, encrypted), by its id: Fly's Machines API takes a
    // volume's id here, and chant passes a declared Volume's name as it is
    // (INTENTIUS/chant#4717). So no Volume is declared, and FLY011 is
    // off (chant.config.ts) until chant resolves the name.
    mounts: [new MachineMount({ volume: "vol_r68lmz2doox9myd4", name: "control_data", path: "/data" })],
    restart: new MachineRestart({ policy: "on-failure", max_retries: 10 }),
    // On a stop, control lets Litestream sync one last time (up to 10 s).
    stop_config: new StopConfig({ signal: "SIGTERM", timeout: "15s" }),
    metadata: { fly_process_group: "app" },
    services: [
      new MachineService({
        protocol: "tcp",
        internal_port: 7690,
        autostop: false,
        autostart: true,
        min_machines_running: 1,
        ports: [
          new MachinePort({ port: 80, handlers: ["http"], force_https: true }),
          new MachinePort({ port: 443, handlers: ["http", "tls"] }),
        ],
        concurrency: new MachineServiceConcurrency({ type: "connections", soft_limit: 6000, hard_limit: 8000 }),
        checks: [
          new MachineServiceCheck({
            type: "http",
            method: "GET",
            path: "/control.json",
            interval: "30s",
            timeout: "5s",
            grace_period: "10s",
          }),
        ],
      }),
    ],
  }),
});
