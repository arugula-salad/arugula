---
section: Added
title: Agents watch a linked repo's GitHub events
line: "An agent can watch a linked GitHub repo's pull requests, checks, reviews and pushes (watch types lists them) and be woken when one happens."
---
`watch` with `event` set to a `github.*` type (a PR opened, closed or merged; its checks done, failed or ready; a check failing on any branch; a review or review comment; a push) and `match` naming the repo, PR or branch fires once when the event comes in through Arugula's GitHub App. A fork's or a non-member's event wakes only a watch whose match asks for `outside: true`. Also in the catalog: a native issue labelled, a forge draft answered, and a machine's daemon updated.
