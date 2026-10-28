# arugulad-hosted

The daemon in a hosted workspace's box (`arugulad --hosted`, M180), split out
of `arugulad-base` (#232) so an edit here rebuilds this crate and the binary,
not the daemon:

- `src/lib.rs`: a box of its own (`PER_BOX`, `fresh_box`), the box's half of
  a hosted daemon's start, and `register`;
- `src/enroll.rs`: enrolling with a ticket or the sandbox id, and trying
  again;
- `src/setup.rs`: the box's setup on its first boot, in a pane, and
  `src/ops.rs`, which says how that went (`hosted_setup.get`, as
  `Handle<HostedBox>`, and the archive of what a rebuild would lose);
- `src/snapshot.rs` (M263): a builder wipes itself after its setup (`_seal`),
  and a box from the snapshot clears what's left at its first start;
- `src/reseed.rs` (`_reseed`), `src/resume.rs`, `src/resolv.rs` and
  `src/activity.rs`: fresh randomness per box, redialling after a resume,
  resolv.conf's newline, and saying busy or idle.

`register()`, which the binary always calls, installs the base's
`hosted::Hosted`, adds its routes through `features` and its programs
through `subcommands`. The base keeps what its runs and agent blocks need:
`hosted/` (the flags, the runs' grant check, the archive of closed agent
blocks, and `trust.rs`, which checks what control sends at enrollment and
at each refresh).

The code here still names what it uses as `crate::…`, as it did when it was
the daemon's: `lib.rs` brings those in. Like every library here it's version
0.0.0.
