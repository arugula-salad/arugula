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
  MachineMetrics,
  MachineMount,
  MachinePort,
  MachineRestart,
  MachineService,
  MachineServiceCheck,
  MachineServiceConcurrency,
  StopConfig,
  Volume,
} from "@intentius/chant-lexicon-fly";

export const controlApp = new App({ name: "illogical-control", org_slug: "jake-gaylor" });

// The hub's SQLite database (Litestream replicates it). It exists already
// (vol_r68lmz2doox9myd4, made with the app): an apply finds it by name and
// leaves it alone, and creates it only in an app that has none.
export const controlData = new Volume({ name: "control_data", region: "ewr", size_gb: 10, encrypted: true });

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
      // Prometheus metrics on the private network, which `metrics` below
      // has Fly's managed Prometheus scrape (part of Arugula #532).
      ARUGULA_METRICS_LISTEN: "[::]:9091",
      // What `fly deploy` sets, kept so `fly status` and a hotfix's deploy see the same Machine.
      FLY_PROCESS_GROUP: "app",
      PRIMARY_REGION: "ewr",
    },
    guest: new MachineGuest({ cpu_kind: "shared", cpus: 1, memory_mb: 1024 }),
    // By name: chant looks the volume's id up at apply time, the volume
    // already attached to this Machine (INTENTIUS/chant#4717). The plan keeps
    // the name, so it's the same bytes whichever id the name resolves to.
    mounts: [new MachineMount({ volume: "control_data", name: "control_data", path: "/data" })],
    restart: new MachineRestart({ policy: "on-failure", max_retries: 10 }),
    // On a stop, control lets Litestream sync one last time (up to 10 s).
    stop_config: new StopConfig({ signal: "SIGTERM", timeout: "15s" }),
    // The launch dashboard's (docs/launch-runbook.md), at fly-metrics.net.
    metrics: new MachineMetrics({ port: 9091, path: "/metrics" }),
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
