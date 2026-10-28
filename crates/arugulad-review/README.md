# arugulad-review

The daemon's views for checking an agent's work (M11), split out of
`arugulad-base` (#232) so an edit here rebuilds this crate and the binary,
not the daemon:

- `src/diff.rs`: the diff block, what changed in a git repository (with
  which chant decision made each hunk, through Labs' `FileWhy`, #619);
- `src/file.rs`: the file block, one file followed live at a line.

What they share (`Runner`, `Live`, the block log) stays in the base's
`review` module, since the forge and Labs use it too. `src/lib.rs`'s
`register`, which the binary always calls, adds the two block kinds through
the base's `features` hooks; the daemon names nothing in here.

The code here still names what it uses as `crate::…` and `super::…`, as it
did when it was the daemon's: `lib.rs` brings those in. Like every library
here it's version 0.0.0.
