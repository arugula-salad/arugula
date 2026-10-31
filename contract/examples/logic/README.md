# Example files for the shared logic

The hosted server (`crates/platform`, `crates/control`) and the daemon both
call into `arugula-e2e` for logic, not just types: who may sign what, which
certificates an account trusts, what a deploy's owner approves, the text of a
driver's plan, a run's moves. These files pin that logic as input → output,
refusals included (#220), so a second implementation can be checked against
them without a Rust toolchain.

`crates/e2e/tests/logic` writes them and checks them. Don't edit them by
hand:

```sh
cargo nextest run -p arugula-e2e --test logic                            # check
ARUGULA_WRITE_EXAMPLES=1 cargo nextest run -p arugula-e2e --test logic   # write, after a deliberate change
```

The check fails if a file isn't what the code makes now, byte for byte. It
also fails if a case's output isn't what its input gives when run from the
file alone, or if a file has an unknown field, a case with no `about`, or a
label no object has. A file no runner knows fails too.

## A file

`<module>/<name>.json`, one check each:

| field | what |
|---|---|
| `about` | what the file checks, in words: the rules, as the Rust code applies them |
| `checks` | the Rust functions it pins, by path in `arugula_e2e` |
| `input`, `output` | what a case's input and output hold |
| `keys` | the fixed keys, by label: `seed` (the Ed25519 seed), `sign` (its public key), `noise`, `device` (the device id) |
| `objects` | the signed objects the cases name (certificates, revocations, rosters, invites, ...), by label, as the Rust types serialize them |
| `cases` | `{ about, input, output }`, in order |

An input names objects by label. A list of labels is a list of those objects,
in that order: order can matter, as in which of two certificates for one
device counts.

Keys come from fixed seeds: device *n*'s Ed25519 seed is the byte *n* 32
times, and its Noise public key is the byte *n* 32 times (nothing here uses a
Noise key except to derive the device id, so there's no private half). Keys
that aren't listed in `keys`, such as a long chain's links, say how they're
made in the file's `about` or in the generator. Times are fixed milliseconds.
Nothing is random. These are made-up keys, so `.gitleaks.toml` skips this
directory.

Every signature is Ed25519 over the exact text the `bodies` files show (each
first line is a frozen domain), hex-encoded. Ed25519 is deterministic, so
signing the same body with the same seed gives the same bytes. An
implementation that signs (the hub signs hosted machines' certificates) can
check its signatures byte for byte.

## The files

- `cert/`: `verify_hex` (strict Ed25519: malleated and small-order
  signatures included), devices' `kinds`, the desktop app's devices
  (`app_names`), certificate and revocation `bodies`, `check_form`, and which
  devices an account trusts (`evaluate`, bad chains, wrong roots and
  revocations included). Also why a certificate is refused (`refusal`) and
  whether a device is a browser all the way to its root (`browser_chained`).
- `team/`: who may sign a paper that raises someone (`raise_ok`, the decision
  table; `raised`), which roster may follow which (`follows`, presigned
  invites and every way to bend one included), the papers' `bodies`, *Ask me
  first* invites (`ask_invite`), a daemon's team `join`, a machine's `move`,
  and a member's `devices`.
- `deploy/`: a deploy's `operation` (its bytes and hash) and `approval` (its
  signed bytes, form and passkey challenge), what a machine checks before it
  applies one (`check_apply`: every refusal, in order, including CLI, app,
  laundered, revoked and grandfathered-app devices, and bad passkeys), which
  `secrets` a driver takes, and `planter`'s and `apps_platform`'s names and
  sources.
- `drivers/`: every driver's `settings` and `setting_fields`, the
  `plan_text` an owner approves (every driver: apply, roll back and
  refused), `planned` (the box's check that the signed operation holds this
  plan), and the source and evidence `checks`.
- `run/`: the eight states and their moves (`state`), the `event` a daemon
  signs (each field bound, ids that would read as another event's), the
  operation's bytes (`op`: encode, and decode only from exactly those
  bytes), and whole runs replayed event by event (`replay`). The replay
  covers grants, approvals, seq replays, another run's or daemon's
  reports, reporters out of their lane, and dead ends. Its rules are the
  platform's `move_run_in` over these types, minus the detail's own rules.
- `hosted/`: the papers' `bodies` and the delegation id, what the hub
  `sign`s (a machine's certificate with control's hosted key, from its
  seed), which client `accept`s a machine (every refusal: form, wrong
  project, not in it, unsigned by an owner's device, a bad signature,
  revoked by time or by another owner), `live`, and project `owner` names.
- `passkey/`: an account's passkey log (DK3, aru-126): each event's `bodies`
  (text, proof line, hash and the challenge a proof signs), and `verify`,
  the one check of a passkey's assertion against a log (the chain, whether
  the passkey was live then, and every way an assertion misses), for
  ES256, Ed25519 and RS256 keys. The page's copy
  (`web/src/shell/e2e/passkey.ts`) runs these files too
  (`web/scripts/passkey.test.mjs`). ES256 signatures are RFC 6979's, so
  deterministic too; the one RS256 assertion is a constant, made once with
  a key that was thrown away.
