# arugulad-forge

The daemon's GitHub forge, split out of `arugulad-base` (#232) so an edit
here rebuilds this crate, Labs and the binary, not the daemon: pull requests
and issues as forge blocks (M36–M38, read with the person's `gh`, agents
draft and people send), project issues pinned from the platform (`native.rs`,
M203) and its mirror (`pin.rs`, `platform.rs`), live updates through
control's GitHub App (`live.rs`, `shadow.rs`), and control's `forge.send`
op (`send_op.rs`). The reader and the model are `arugula-forge-github`'s
([crates/forge-github](../forge-github/README.md)).

The daemon reaches it only through its `forges.rs`, the `Forges` trait:
each call's default is what a build without the forge does, nothing or "X
isn't in this build". This crate's `register` installs its impl, and the
binary ([crates/arugulad](../arugulad/README.md)) calls it when its `forge`
feature is on, which it is by default.

Labs' Forgejo and GitLab are built on it (`arugulad-labs` depends on this
crate): a forge block asks for them through `others.rs`, the
`OtherForges` trait, which Labs registers; its defaults say a build
without Labs hasn't them.

Its code names the daemon's modules as `crate::…`, as it did when it was the
daemon's (`lib.rs` brings them in). Like every library here it's version
0.0.0.
