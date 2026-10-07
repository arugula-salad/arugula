# arugula-control-wire

The JSON that daemons and Arugula control send each other to enrol, to learn
their account's and team's certificates, to say who gets in, to dial the
relay, to relay web push, to get ICE servers for huddles and to hear about
GitHub: one type per message, so a renamed field breaks both builds. The CLI
joins with these types too. The routes are `pub const`s here too.

Depends on `arugula-e2e` (certificates, rosters, push subscriptions) and
serde. Not on `arugula-proto`: control shouldn't pull in the client wire types.

Start with `src/lib.rs`: its header says what must stay true of every type
(the bytes on the wire don't change; deployed daemons and control upgrade
separately). `src/join.rs` is enrolment, `src/team.rs` the certificates,
`src/forge.rs` the GitHub App's relay messages and tokens.
