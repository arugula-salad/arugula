---
schema: 1
id: "aru-069"
title: "Zig 0.16 builds Ghostty; the toolchain is pinned"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Zig 0.16 builds Ghostty; the toolchain is pinned"
options:
  - id: "a"
    label: "Zig 0.16 builds Ghostty; the toolchain is pinned"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S5"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:.mise.toml"
  - "path:rust-toolchain.toml"
---

# Zig 0.16 builds Ghostty; the toolchain is pinned

libghostty-vt needs Zig, pinned in `.mise.toml`; cargo runs under `mise exec`
(the `just` recipes do it), and the daemon embeds `web/dist`, so `just web`
comes before any cargo build.
Where: `.mise.toml`, `justfile`, `rust-toolchain.toml`.
From: [S5](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), M2.
