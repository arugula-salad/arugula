# Stored files

Files a daemon keeps in its state directory, as a release wrote them, so a
rename in the Rust types can't stop them loading or drop a field. Each
crate's test reads its file and writes it back unchanged:

| File | Where a daemon keeps it | Test |
|---|---|---|
| `layout.json` | `<state>/layout.json`, saved by arugulad 0.67.0, with paths, names and ids replaced | `arugula-mux`'s `store::tests::release_layout_loads` |
| `work-session-ledger.json` | `<state>/work/sessions/ses_….json` on a hosted machine, every field set | `arugulad-base`'s `work::sessions::tests::release_ledger_loads` |
| `mentions.json` | `<state>/mentions.json`, every field set | `arugulad-base`'s `agent_mentions::tests::release_mentions_load` |
| `followups.json` | `<state>/followups.json`, every field set (#630) | `arugula-mux`'s `mux::followups::tests::stored_followups_load` |
| `sharing.json` | `<state>/sharing.json` | `arugula-proto`'s `sharing::tests::release_sharing_loads` |
