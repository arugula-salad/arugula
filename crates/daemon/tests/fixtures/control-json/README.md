# `control.json` as the code before #672 wrote it

One file per kind of machine and purpose, as `crates/daemon/src/control.rs`
wrote them before a machine's kind was one type (Arugula #672), with what
that code decided from each beside it. The test
`control::tests::control_json_from_before_the_kind_was_one_type_loads_and_decides_the_same`
loads each, writes it back as the same JSON, and checks it decides the same.

| File | What it is |
|---|---|
| `personal-joined` | Just joined to its account: what `join` saves |
| `personal` | After refreshes: a removed device, a login, a move back from a team, a person (`s`) and a team (`t2`) sessions were shared with, a person and a team control left out |
| `team-joined` | Just moved into team `t1`: the pin, no roster yet |
| `team` | The roster checked, members' certificates (one of `e`'s devices removed), their names, `x` taken out of the team |
| `team-locked` | The same, locked by an owner's signed word |
| `hosted-account` | A people's workspace of an account's project, with pinned passkey logs |
| `hosted-team` | A people's workspace of a team's project |
| `hosted-team-locked` | The same after two refreshes: `x` taken out between them, and the team locked |
| `hosted-deploy` | A deploy's box of an account's project |
| `hosted-deploy-team` | A deploy's box of a team's project |
| `hosted-snapshot` | A snapshot's box of a team's project |
| `hosted-both` | A deploy's box told at a refresh that it's a snapshot's: both flags set |

Beside each `<name>.json`:

- `<name>.who.json`: what the code decided from it, as the test's `outcomes`
  function prints it: who's trusted, who gets in and as whom, who drives it
  and calls its agents, who may sign for its project, the pinned passkeys,
  and what it says of itself and of each account.
- `<name>.accept.json`: other hosted machines' certificates, under
  delegations for `t1` (signed by an owner, and by an editor), `t2`, `t9`,
  account `a` (by its root, and by a stranger) and account `acct1`, and what
  `peer::accept_hosted` said of each (`ok <project>` or `err`).

## How they were made

Not by an old release: by the code on `origin/main` at
`254319777fe406e7913d973134ffa98a1fc5e55c` (2026-11-03), the last commit
before #672, whose `Saved` still had the kind spread over `team`, `roster`,
the team's fields and `hosted` with its `deploy` and `builder` flags. That
code wrote the files through its own `serde_json::to_vec_pretty` and the
decisions through the same `outcomes` function the test uses now, written
against the old types. The keys are made for the fixtures and their private
halves were never kept; the files hold public keys and signatures only.

To make them again, from a clean checkout of that commit:

```sh
git worktree add /tmp/arugula-main origin/main   # at the commit above
cd /tmp/arugula-main
git apply crates/daemon/tests/fixtures/control-json/generate.patch
cargo nextest run -p arugulad-base --run-ignored only -E 'test(zz_write_control_json_fixtures)'
```

`generate.patch` is the generator: the ignored test and the `outcomes`
function on the old types, and `peer::accept_hosted` made `pub(crate)`. Every
run makes new keys, so the files differ each time; what they decide doesn't.
