# arugula-platform-deploys

Deploys (#314), on the platform's base (`arugula-platform-base`,
[crates/platform](../platform/README.md)):

- `src/deploys.rs`: a project's environments, the deploys planned into
  them, and their approval with an owner's passkey (M217, M218). The
  verifier, the only maker of a `VerifiedApproval`, is the base's
  (`approvals.rs`), beside the type, so the compiler keeps it the only one. `src/deploys/agent.rs` is a machine's agents'
  routes (plan and ask, never approve), `src/deploys/catalog.rs` the
  drivers a project may name.
- `src/deploys/apply.rs`: applying an approved deploy on a box of its own
  through the runs engine (M219): the job, the box's secrets and artifact,
  its reports, and the reaper.
- `src/deploys_tests.rs` and `src/failure_tests.rs`: the routes end to
  end, and every failure an apply can meet (docs/control-ops.md).

The drivers (`drivers/`) and the deploys' tables (`db/deploys*.rs`) stay
in the base: agents with no machine, redone's import and the forge ask
them too. The rest of the platform reaches deploys through
`features::Deploys` (the runs engine, a deploy box's asks, the agents' MCP
tools), and the crate adds its routes, its job and the reaper through
`features::Feature`. Its tests: `scripts/server-cargo nextest run -p
arugula-platform-deploys`. Its `testing` feature exports `deploys_tests`'
helpers (signed approvals, a CLI's gate) to other crates' tests.
