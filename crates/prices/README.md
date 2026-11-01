# arugula-prices

What Claude tokens cost at Anthropic's list prices: one table, shared by the
platform's broker (which settles hosted calls at it, `crates/platform`'s
`hosted/broker/prices.rs`) and the daemon (which estimates what a Claude Code
session and its subagents cost from their transcripts, #398,
`crates/arugulad-agent`'s `conversations/spend.rs`). It's in the root
workspace and reached from the server's by path, like
`arugula-control-wire`.

Start at `src/lib.rs`: `PRICES`, `lookup`, `Usage` and
`cost`. A model the table doesn't name has no price here; the broker charges
it as the dearest row, and an estimate shows its tokens without dollars.

When Anthropic adds a model or changes a price, change the row against
<https://platform.claude.com/docs/en/about-claude/pricing> and its tests.
