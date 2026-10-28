# arugulad-docs

Docs (#963, #1493, #1066), split out of `arugulad-base` (#232) so an edit
here rebuilds this crate and the binary, not the daemon:

- `src/docs/`: the Doc block, its library (`library.rs`), its pictures
  (`assets.rs`), the fenced charts, tiles and drawings it checks
  (`embeds.rs`), sharing a link (`link.rs`) and an agent's ask to share
  (`ask.rs`).
- `src/ops.rs` and `src/links.rs`: the operations (`docs.list`, `doc.open`,
  `doc.delete`, `doc_link.get`, `doc_link.delete`), as `Handle<Docs>`.
- `src/tool.rs`: the `doc` MCP tool, listed after `show`.

`src/lib.rs` registers it all: a `features::Feature` (the block kind, the
routes, the library's migration before the layout is restored, control for
links, the library's lines in `search`) and the tool. The words of who may
share a Doc are the base's (`doc_shares.rs`), whose API checks a Doc
block's sharing methods with them.

Its tests read `crates/daemon/tests/fixtures/docs/`, which the web client's
specs read too.
