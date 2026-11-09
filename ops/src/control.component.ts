// Control's hub (illogical-control), released to production with each
// release, from the image the release names (CI builds and pushes
// it: .github/workflows/server-release.yml).
//
// The hub holds every relayed terminal, and a deploy drops them all, so a
// release deploys it only when something it's built from changed since the
// release production runs it from (`inputs`, INTENTIUS/chant#4695; this
// replaced scripts/ci-hub-changed). The plan, dist/control.fly.json, is
// compared too, whether listed or not. A version number alone isn't a
// change: the manifests and lock files are compared after `scripts/release
// blank`, which writes every arugula crate's version as @V@
// (INTENTIUS/chant#4718). `--deploy-unchanged` deploys it anyway.
//
// `fly-release` waits for the Machine's Fly check (/control.json) after the
// update and verifies it, and puts the config it replaced back if either
// fails (INTENTIUS/chant#4719), so a hub that doesn't come up healthy goes
// back to the release before it, as scripts/ci-fly-rollback did.
import { phase, type Component } from "@intentius/chant/components/component";

// Every crate arugula-control compiles (it runs the platform in process, so
// that's the platform's too), less their tests.
const crates = [
  "arugula-control",
  "control",
  "control-common",
  "control-wire",
  "core",
  "e2e",
  "forge-github",
  "platform",
  "platform-agents",
  "platform-all",
  "platform-api",
  "platform-chat",
  "platform-deploys",
  "prices",
  "proto",
].map((c) => `:(top)crates/${c}`);

export const control: Component = {
  name: "control",
  // After the platform, in a job of its own (each app has its own Fly token).
  dependsOn: [],
  inputs: {
    paths: [
      ...crates,
      ":(top,exclude,glob)crates/**/*_tests.rs",
      ":(top,exclude,glob)crates/*/tests/**",
      // web/'s sources for the pages it embeds (web/dist, web/dist-platform),
      // less the specs, their fixtures and the dev and test servers.
      ":(top)web",
      ":(top,exclude)web/e2e",
      ":(top,exclude)web/fixtures",
      ":(top,exclude)web/screenshots",
      ":(top,exclude)web/safari",
      ":(top,exclude,glob)web/**/*.test.*",
      ":(top,exclude,glob)web/**/*.fixture.*",
      ":(top,exclude,glob)web/playwright*.ts",
      ":(top,exclude)web/safari.config.ts",
      ":(top,exclude)web/screenshots.config.ts",
      ":(top,exclude)web/control-smoke.ts",
      ":(top,exclude,glob)web/dev-*.ts",
      ":(top,exclude)web/e2e-interop.ts",
      ":(top,exclude)web/fake-fleet.ts",
      ":(top,exclude)web/reconnect-watch.ts",
      ":(top,exclude,glob)web/scripts/e2e-*",
      ":(top,exclude,glob)web/scripts/perf-*",
      // control's build.rs serves editors/nvim as a git repository.
      ":(top)editors/nvim",
      // How the binary and its image are built. Version numbers alone don't
      // count (`normalize`, below).
      ":(top)Cargo.toml",
      ":(top)Cargo.lock",
      ":(top)server/Cargo.toml",
      ":(top)server/Cargo.lock",
      ":(top,glob)crates/*/Cargo.toml",
      ":(top)rust-toolchain.toml",
      ":(top).cargo/config.toml",
      ":(top).mise.toml",
      ":(top)scripts/zig-cc-musl",
      ":(top)scripts/zig-ar",
      ":(top)scripts/server-cargo",
      // The normalizer itself: a change to how versions are blanked deploys.
      ":(top)scripts/release",
      ":(top).github/actions/setup",
      ":(top).github/workflows/server-release.yml",
      ":(top)justfile",
      ":(top)packaging/control",
      // Its own steps and Machine.
      "src/control.component.ts",
      "src/fly/control",
    ],
    // Each changed manifest or lock file is read at both commits and compared
    // after this, so a release's version numbers alone leave the hub alone.
    normalize: [
      {
        paths: [
          ":(top)Cargo.toml",
          ":(top)Cargo.lock",
          ":(top)server/Cargo.toml",
          ":(top)server/Cargo.lock",
          ":(top,glob)crates/*/Cargo.toml",
        ],
        command: "scripts/release blank {path}",
      },
    ],
  },
  deploy: [
    phase("Image", [{ kind: "release-image" }]),
    phase("Release", [
      {
        kind: "fly-release",
        plan: "dist/control.fly.json",
        machine: "control",
        image: "@Image.uri",
        digest: "@Image.digest",
        verify: { url: "https://control.arugula.io", healthPath: "/control.json" },
        // Wait for the Machine's Fly checks after the update, five minutes
        // at most, and restore it when they don't pass.
        checks: true,
      },
    ]),
  ],
};
