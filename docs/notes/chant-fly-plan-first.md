---
section: Changed
title: Approve the plan chant made
line: A chant-fly deploy now runs chant's plan before anyone approves it, and your passkey signs that plan, so the repo needs chant 0.124.64 or newer and your passkeys in .chant/trust.json.
needs: control
---
Asking for a chant-fly deploy's approval now starts a plan run on a box of its
own, with no Fly token. The commit is fetched with a GitHub token that only
reads, chant runs the op to its gate with no token at all and records its
plan, and only once everything the op ran is stopped does the box get a token
that writes, to push chant's ledger. Owners are asked once the plan is there,
and the approval page shows chant's gate and plan digest. Your passkey signs
chant's own gate statement for that plan, and on the apply box chant checks the
signature itself against `.chant/trust.json`, after checking the plan isn't
stale.

What a repo needs before Arugula can deploy it this way:

- `@intentius/chant` 0.124.64 or newer in its `package.json`. The driver
  refuses an older chant and says which version the repo pins.
- Each approver's passkeys in `.chant/trust.json`, as `arugula:<account>`.
  `arugula chant trust-entry` prints the entries to paste in. Keep a signers
  file there too, even an empty one.

A chant-fly deploy asked for before this has no chant gate: plan it again. An
environment that held its own `GITHUB_TOKEN` and has no GitHub App installed
can't plan a chant-fly deploy, since the plan run's token is always minted.
Roll backs and the other drivers are approved as before.
