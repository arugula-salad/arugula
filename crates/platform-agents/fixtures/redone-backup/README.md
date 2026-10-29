# A redone backup, for `import redone`'s tests (M235)

`state.json` is shaped as redone's `store.mjs` writes it (arugula-salad/redone at a4b1b49):
`{ tenants, tokens, apps, uploads, plans }`, with tokens and approver keys kept only as
their SHA-256 (hex), tokens keyed by it. The tenants, apps and plans are made up.

`tokens.txt` is the made-up `rdn_` token behind each hash, so the tests can call the
platform's MCP endpoint as redone's agents would. They're test strings, never real tokens.

- `fern`: one person; an ntfy topic and auto-approve-updates (both not carried over);
  two tokens; apps `fern-site` (two deploys) and `fern-blog` (a pending plan only).
- `moss`: two people and no team: refused until the operator names one.
- `alder`: a team's tenant; one token; app `alder-api`.
- `newbie`: its person isn't on Arugula yet: waits.
- `lonely`: nobody said whose it is: waits.
- `tok_ghost1`: a token of a tenant that isn't in the backup.
