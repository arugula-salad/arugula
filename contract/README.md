# contract

The wire contract between Arugula's daemons and CLI, control's hub and its
platform, written down where any implementation can check itself against it
(#219). The Rust types in `crates/control-wire` are the source: everything
here is generated from them or tested against them.

- `schema/`: JSON Schema (draft 2020-12) for every type in
  `crates/control-wire`.
- `examples/logic/`: inputs and answers for the shared security logic
  (#220): its README says what's there.

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
