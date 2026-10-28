# arugulad

The daemon's binary. `src/main.rs` marks the release in the binary's bytes
and calls `arugulad_base::main` ([crates/daemon](../daemon/README.md)), where
the daemon is. As the daemon splits into feature crates (#232), this is where
they're wired in. Its cargo features `labs`, `forge` and `editor` each
register a feature crate (`labs` turns `forge` on); `debug-embed` is the
base's.

Its integration tests are one binary, `tests/integration/`, on
`arugula-testkit`: each starts `arugulad` (`CARGO_BIN_EXE_arugulad`) and
drives it from outside. They read their recorded data and fake agents from
`crates/daemon/tests/` ([docs/testing.md](../../docs/testing.md)).
