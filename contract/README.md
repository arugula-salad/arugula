# contract

The wire contract between Arugula's daemons and CLI, control's hub and its
platform, written down where any implementation can check itself against it
(#219). The Rust types in `crates/control-wire` are the source: everything
here is generated from them or tested against them.

- `schema/`: JSON Schema (draft 2020-12) for every type in
  `crates/control-wire`.
- `examples/wire/`: real payloads of those types, which decode and encode
  again without a byte changing.
- `examples/logic/`: inputs and answers for the shared security logic
  (#220): `examples/logic/README.md` says what's there.

## schema/

One file per module of `crates/control-wire`: `chat.json` is
`arugula_control_wire::chat`, `lib.json` the crate's root (`/control.json`,
joining, the relay's dial), and `internal.json` the internal port: what the
hub and the platform say to each other over the private network
(`HubPort`/`PlatformPort`'s arguments and answers, and the principal header's
JSON). Each file's `$defs` holds the module's public types by their Rust name,
and every type they carry, `arugula-e2e`'s certificates, rosters and grants
among them. A type that two modules carry appears in both files.

The schemas describe what a reader accepts (schemars' deserialize
contract), which is the contract's leniency written down:

- A field read with `#[serde(default)]` isn't in `required`.
- No schema sets `additionalProperties: false`: nothing in control-wire is
  `deny_unknown_fields`, and a reader ignores a field it doesn't know.
- An enum with `#[serde(other)]` (`ChangeKind`'s `Unknown`) takes any
  string, as the Rust reader does.

What a schema can't say: a hand-written `Serialize` (`JoinPoll` leaves out
the fields an unapproved join doesn't have) and the `t` tags (`ActivityTag`,
`forge`'s), whose schemas are written by hand (`const`).

**Writing it:** `just contract-schema`. **Checking it:** `just contract-schema
check`, which CI runs on every pull request and on main
(`.github/actions/lint`). So a change to a wire type is a diff here, in the
same pull request, for the reviewer to read.

**A new type:** give it `#[cfg_attr(feature = "schema",
derive(schemars::JsonSchema))]` beside its serde derive and add it to its
module's list in `crates/control-wire/src/schema.rs`. The test
`every_wire_type_is_in_the_schema` fails without either.

## examples/wire/

`examples/wire/<module>.<Type>/` holds payloads of one type: `internal.Team/`
is `arugula_control_wire::internal::Team`, and `$defs/Team` in
`schema/internal.json`. They come from the messages control-wire's tests
recorded from both sides, or are built the way the hub's and the platform's
code builds them, with obviously fake ids, keys and tokens.

- **`<name>.json`** is a payload as it's sent. Decoded as its type and
  encoded again, it gives back exactly its bytes.
- **`<name>.lenient.json`** is a payload a reader has to take although no
  up-to-date side sends it: a field it doesn't know (from a newer peer), one
  an older peer left out, keys in another order, a value an enum doesn't
  know. It decodes, and encodes to exactly `<name>.json`'s bytes.

`crates/control-wire/src/examples.rs` checks both. It also checks that each
directory names a type it knows, and that each type is in the schema.

### The round-trip rule

"The bytes don't change" can't mean that any JSON a reader takes comes
back byte for byte: the reader is lenient on purpose, so many inputs decode
to the same value. What the test holds to is this:

1. **What a side sends doesn't change.** An encoding is compact JSON
   (`serde_json::to_string`): no whitespace, a struct's fields in the order
   they're declared, a map's keys (and those of free-form JSON, a
   `serde_json::Value` such as `PushArgs.message`) sorted, and an absent
   `Option` as `null` unless the field is skipped when empty. A
   `<name>.json` is exactly that, so any change to what's sent (a renamed
   field, a reordered one, a `null` that's now left out) fails its test.
   Deployed peers parse these bytes, and they don't upgrade together.
2. **What a reader takes, and what it makes of it, doesn't change.** A
   `<name>.lenient.json` encodes to its `<name>.json`, which pins down how
   it's normalized:
   - an unknown field is dropped;
   - a missing field read with `#[serde(default)]`, or a missing `Option`,
     comes back as its default;
   - keys come back in the order of rule 1;
   - a value an enum with `#[serde(other)]` doesn't know comes back as that
     variant's own name (`ChangeKind`'s `"team.renamed"` as `"Unknown"`,
     telemetry's `"plan9"` as `"other"`). This is the one case where reading
     loses a value, not just a field. The variant says "something newer",
     and a reader acts on that alone.

Sorted keys in free-form JSON hold because neither workspace builds
`serde_json` with `preserve_order`. If a crate turns it on, the
`PushArgs` example fails. That's on purpose: the bytes would then depend
on how each sender built the object.

**For another implementation:** decode every file; encode each
`<name>.json`'s value back to its exact bytes; and treat a
`<name>.lenient.json` as input only, with its `<name>.json` as the
expected encoding. Its schema says what's allowed, and these files say
what's actually sent.

**Exceptions the schema shows:** three `boards` types (`Diffstat`,
`BlockState`, `PutBlockState`) are `deny_unknown_fields`, against
control-wire's rule, so their schemas are closed (`additionalProperties:
false`). An externally tagged enum's wrapper object (`TokenAnswer`'s
`{"token": ...}`) takes no other key, because serde's reader doesn't.

**A new example:** write the payload on one line, as it's sent, in the
type's directory (add the type to `types()` in `examples.rs` if it's new).
When it doesn't round-trip, the test prints what it encodes to.
