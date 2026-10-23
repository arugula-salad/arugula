# planter, as GitHub has it

Real files from planter's GitOps repositories, captured 2026-10-23 for the
planter drivers' tests (M233, `crates/daemon/src/work_planter_tests.rs`):
the box's half is held to what planter actually wrote, not to what a test
thinks it wrote.

- `planter-apps/redone-hello.yaml`, `planter-apps/ocarina.yaml`: the pointers
  in `arugula-salad/planter-apps` at 4bf93dcc (planter's ship's, and its Go
  live's).
- `shipped/redone-hello/`: `arugula-salad/shipped`'s `redone-hello/` at
  a7164c3 (planter's ship, then the build's pin); `redone-hello.last-commit`
  is the message of the last commit to its source (planter's ship, with its
  `Shipped-By` and `Ship-Token` trailers).

planter's renderer output is in `crates/e2e/src/planter/fixtures/`
(`scripts/vendor-planter`).
