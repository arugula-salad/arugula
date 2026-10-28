# arugulad-labs

Labs: the daemon's features that aren't part of the core. Studio apps with
hud, chant workspaces, VMs (the Sprites and loam providers, resident daemons
and the provider tunnel, the tailnet sandbox supervisor), guest ssh, agent
recipes with the agent catalog and delegating, and the Forgejo and GitLab
forges. The runtime switches are Labs' flags in the state directory
(`arugulad-base`'s `lab_flags.rs`); see [docs/labs.md](../../docs/labs.md).

The daemon (`arugulad-base`, [crates/daemon](../daemon/README.md)) reaches
it only through `lab_hooks.rs`, the `Hooks` trait: each method's default is
what a build without Labs does, nothing or "X isn't in this build". This
crate's `register` installs its impl, and the binary
([crates/arugulad](../arugulad/README.md)) calls it when its `labs` feature
is on, which it is by default. Its operations are `Handle<Labs>`, routed with
`op_of`. So an edit here rebuilds this crate and the binary, not the daemon
(#232).

Its code names the daemon's modules as `crate::…`, as it did when it was the
daemon's (`lib.rs` brings them in). Test fixtures stay in
`crates/daemon/tests/fixtures`.

Like every library here it's version 0.0.0.
