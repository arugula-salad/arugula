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

Not here: the internal port, what the hub and the platform say to each other
over the private network (`HubPort`/`PlatformPort`'s arguments and answers,
and the principal header's JSON). Only those two send it, so its messages,
schema and examples live in `crates/control-common` (`src/port/`, `port/`),
checked by that crate's tests the same way.

## Compatibility

The sides of the contract don't upgrade together: control deploys on its
own, a daemon (and its CLI) when its owner updates it, a native app when its
build ships. So the contract has to work for every **supported version**
(Arugula #807, decided 2026-11-16):

- any daemon released in the last 30 days. The train releases about five
  times a day, so "the current release and the one before" would cover
  hours;
- the current and the previous build of each native app.

**Changes are additive.** A new route, operation, type, optional field or
enum value is fine. A route, operation, type, field or value is removed, a
field made required (a new required field on a type that's already there
included), or a field's type changed (made nullable included), only once no
supported version reads or sends it.

**Removing something** takes two steps:

1. Deprecate it: nothing new relies on it, but it's still sent and still
   read. Its doc comment says so, with the release from which nothing needs
   it.
2. Remove it once the window has passed: the last daemon release that needs
   it is more than 30 days old, and neither native build in support reads or
   sends it. A line in the commit message names what goes, and why that's
   safe now:

   ```
   Contract-break: chat.ChatRoom.archived, last read by 0.70.3 (released 2026-11-14)
   ```

**Checking it:** `just contract-compat` (`scripts/contract-compat.mjs`)
compares `contract/` in the working tree with `contract/` at its merge-base
with origin/main. It fails on a removed route (or a client a route stops
serving), a removed operation or one that now takes or answers another type,
a removed type, field, enum value or variant, a field that became required
or was added as required, and a changed type (its JSON types, `$ref` or
`format`). Additions pass. It names each finding by an id: `chat.ChatRoom.archived`,
`chat.ChatLive<type=hello>`, `GET /api/chat/rooms`. A `Contract-break:` line
in any commit message on the branch allows the finding it names and
everything under it (`chat.ChatRoom` covers its fields). `just check` runs
it; CI doesn't yet (Arugula #1139). Its tests are
`scripts/tests/contract-compat.sh`.

The schema can't say everything: a change can keep every field and still
break an older peer (a field it now needs set, a route whose meaning
changed). `just older-daemons` (`scripts/older-daemons.mjs`, Arugula #808)
runs this checkout's page against the oldest and the newest daemon release
of the window.

One addition has an order: a field on a closed schema (`additionalProperties:
false`, below) ships in control and deploys before any daemon sends it.

## schema/

One file per module of `crates/control-wire`: `chat.json` is
`arugula_control_wire::chat`, and `lib.json` the crate's root (`/control.json`,
joining, the relay's dial). Each file's `$defs` holds the module's public types by their Rust name,
and every type they carry, `arugula-e2e`'s certificates, rosters and grants
among them. A type that two modules carry appears in both files.

The schemas describe what a reader accepts (schemars' deserialize
contract), which is the contract's leniency written down:

- A field read with `#[serde(default)]` isn't in `required`.
- A reader ignores a field it doesn't know, and its schema doesn't set
  `additionalProperties: false`, except for the bodies control parses from
  a daemon and must refuse anything else in (below).
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

## openapi/

`openapi/native.json` is what a native client calls on control (#599): OpenAPI
3.1, generated by `just api-contract` from `api/native.json`, `api/routes.json`
and `schema/`. It isn't a public API (aru-131). `api/routes.json` is
`crates/control/routes.toml`'s rows the contract cares about (in scope, or
listing native), one a line: which routes exist, their auth and who may call
them. `just api-contract` writes it too, so the contract reads only
`contract/`. Each operation's types are
`schema/`'s; one marked `x-arugula-untyped` answers or takes something that isn't
in `crates/control-wire` yet.

**Checking it:** `just api-contract check`, which CI runs on every pull request.
It fails when the document or `api/routes.json` is stale, an operation names a
route `routes.toml` doesn't have or a type `schema/` doesn't, a handler's signature doesn't mention
the types its operation names, or **a route in scope isn't classified**.

**A new route in scope** (chat, projects, issues, boards, sessions, deploys,
runs, the inbox) goes in `api/native.json`: under `operations` with its handler
and types, or matched by a rule in `out` that says why not. A type two modules
define differently is written `module.Name`.

The map and the generator are outside `crates/control-wire` and `routes.toml`
on purpose: the hub's release inputs (`ops/src/control.component.ts`) count a
change to either as a hub change, and a hub deploy drops every relayed terminal.

## examples/wire/

`examples/wire/<module>.<Type>/` holds payloads of one type: `team.AccessList/`
is `arugula_control_wire::team::AccessList`, and `$defs/AccessList` in
`schema/team.json`. They come from the messages control-wire's tests
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

**Exceptions the schema shows:** what a daemon reports of its blocks and
sessions is `deny_unknown_fields` on purpose (aru-116), so no message text,
file name or prompt can ride along: `boards`' `BlockStateReport` (with its
`DiffstatReport` and the `PutBlockState` around it) and
`MachineSessionsReport` (with its `MachineSession` and
`MachineSessionPane`), and `PaneAsksReport` (with its `PaneAsk`). Their schemas are closed (`additionalProperties:
false`). An older control refuses a field these don't list yet, so a new
one ships in control and deploys before any daemon sends it. An externally tagged enum's wrapper object (`TokenAnswer`'s
`{"token": ...}`) takes no other key, because serde's reader doesn't.

**A new example:** write the payload on one line, as it's sent, in the
type's directory (add the type to `types()` in `examples.rs` if it's new).
When it doesn't round-trip, the test prints what it encodes to.
