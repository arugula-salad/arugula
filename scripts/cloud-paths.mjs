// Paths that left this repo for arugula-salad/cloud, closed source, when
// control's hub and its platform moved out (Arugula #844). Docs and
// decision records written before then still name them, and so may new
// ones that describe how the cloud uses this repo; scripts/decisions-index.mjs
// and scripts/data-model-check.mjs take such a path as being in that repo.
export const CLOUD = [
  "crates/arugula-control",
  "crates/arugula-platform",
  "crates/control",
  "crates/control-common",
  "crates/platform",
  "crates/platform-agents",
  "crates/platform-all",
  "crates/platform-api",
  "crates/platform-chat",
  "crates/platform-deploys",
  "server",
  "ops",
  "packaging/control",
  "packaging/platform",
  "packaging/platform-dump",
  "docs/control-ops.md",
  "docs/secrets-inventory.md",
  "docs/platform-features.md",
  "docs/managoat.md",
  "docs/launch-runbook.md",
  "docs/launch-dashboard.json",
  "docs/design/control-email.md",
  "scripts/hub-restore-drill",
  "scripts/dump-age-check",
];

/** Whether a repo path is one of the cloud's, or under one. */
export const inCloud = (p) => CLOUD.some((c) => p === c || p.startsWith(`${c}/`));
