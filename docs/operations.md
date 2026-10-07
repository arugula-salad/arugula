# Operations: declare each one once

A design note for #451 (part of #387). It proposes how one declaration per
operation can produce the HTTP route, the access check for it, the MCP tool
and the CLI's call, and says plainly where that stops working. A pilot on
the branch `arch/451-ops-pilot` converts three real operations; what it
showed is in [The pilot](#the-pilot). The owner settled the open
questions on 2026-10-07 ([Decided](#decided)); the rest waits on the
team's review, and then gets an entry in `DECISIONS.md`.

## What we have today

A capability usually touches five places, each with its own parsing and its
own errors:

- **The types** are in `crates/proto/src/api.rs` (#445, #446), and the web
  client's copy is generated from them (`web/src/proto.gen.ts`, ts-rs roots
  in `crates/proto/src/ts.rs`).
- **The route** is a line in the `Router::new()` chain at the top of
  `crates/daemon/src/api.rs` (or `fs.rs`, `hosts.rs`, `share.rs` …) and a
  handler beside it.
- **Who may call it** is decided twice. `crates/daemon/src/authz.rs` matches
  the method and path against a table (`policy`: the owner only, a role on
  the pane's session, or "the handler checks"), and many handlers check
  again (`owner_only`, `is_invite`, the agent header from
  `crates/core/src/rename.rs`, read by `invite::agent`).
- **The MCP tool** is a `Def` (or a `Kind` row of a grouped tool) in
  `crates/daemon/src/mcp/tools.rs`, an argument struct with
  `Deserialize + JsonSchema`, a result in `mcp/results.rs` (#448), an arm in
  `dispatch`, and a method on `Call`. MCP has its own idea of who may call:
  a token's `Scope` (`Full`, `Read`, or an agent block's `Block(id)`, which
  reads its own tab and drives what it started). MCP doesn't call the HTTP
  routes; it reaches the mux directly, so its handlers are a second copy.
- **The CLI** builds the request by hand (`request(&sock, "POST",
  &format!("/api/panes/{}/close", p.0), None)`), parses the answer with the
  proto type (`parse`, `parse_raw`), and has a clap subcommand in
  `crates/cli/src/main.rs` with its arguments in `crates/cli/src/cmd/*.rs`.

The method, the path and the answer type are written in at least two
places (the daemon and the CLI), and what an operation does is written
twice when it has an MCP tool.

## What one declaration holds

An operation has two halves, because the CLI can't link the daemon.

**The wire half, in proto** (`arugula_proto::op`), which the daemon and the
CLI both see:

```rust
pub trait Op: Send + Sync + 'static {
    const NAME: &'static str;            // "pane.close": for logs and errors
    const METHOD: Method;                // Post
    const PATH: &'static str;            // "/api/panes/{id}/close"
    const ACCESS: Access;                // Pane(Role::Editor)
    const LABS: bool = false;            // listed only with the labs file
    const DRIVES: bool = false;          // types into the pane: needs the owner's trust (M14)
    const CREDENTIAL: bool = false;      // a GET whose answer grants access: not for read-only tokens
    type Path: PathArgs;                 // PaneId, or ()
    type Req: Request;                   // the body; Empty for none
    type Res: Serialize + DeserializeOwned;
}

pub enum Access { Owner, Pane(Role), Handler, Anyone }
```

`Access` is `authz.rs`'s `Policy` without the pane number: the declaration
says which role, and the path supplies the pane.

`NAME` is the operation's own name, `noun.verb` (`panes.list`,
`pane.close`, `shell_env.get`, `shell_env.refresh`), not any surface's: the
same operation is `GET /api/panes`, MCP's `list` kind `panes` and the CLI's
`ls`, and a name tied to one of them goes stale when that surface renames
or drops it. (The pilot used the CLI's names; ticket 1 renames them.) The doc comment on the
declaration is the API's description, the line that is today a row in the
table at the top of `proto/src/api.rs`.

**The daemon half** (`crates/daemon/src/ops/`), one file per operation or
family:

```rust
pub trait Handle: Op {
    fn handle(cx: &Cx<'_>, path: Self::Path, req: Self::Req)
        -> impl Future<Output = Result<Self::Res, OpError>> + Send;
}
```

and, when the operation has an MCP tool, beside it:

```rust
pub trait McpOp: Handle {
    const SURFACE: Surface;              // Tool { name, title, description, … }
                                         // or Kind { tool: "list", kind: "panes", … }
    type Args: DeserializeOwned + JsonSchema;
    fn args(call: &Call, a: Self::Args) -> Result<(Self::Path, Self::Req), String>;
    fn answer(call: &Call, path: &Self::Path, res: Self::Res) -> Out;
}
```

That is the whole declaration: name, route, request and answer types,
required role, labs, the handler, and per surface its text. The CLI's help
stays on its clap command; see [Help text](#help-text).

`Cx` is what a handler knows about the call: the `App`, and which way it
came in (`Via::Http { owner, agent }` or `Via::Mcp(&Call)`). The handler is
written once and asks `Cx` only where the surfaces differ on purpose:

- `cx.may(Self::ACCESS, pane)`: over HTTP, nothing (the middleware already
  checked); over MCP, `Pane(Viewer)` is `Call::readable` (the token reaches
  that tab) and `Pane(Editor)` is `Call::drivable` (the agent started it).
- `cx.agent_or_guest()`: what the owner keeps for themselves, like closing an
  invite block (#234). Over HTTP that's a guest or a request with the agent
  header; every MCP caller is an agent.

`OpError` names why it didn't happen (`NoPane(id)`, `Forbidden(why)`,
`Unreachable(why)`), and each surface words it as it does today: HTTP's
`404 "no pane %7"`, MCP's "pane %7 is gone; its last command `make`
exited 2 3m ago (read_output still reads it)".

## How each surface comes from it

**The route.** `Router::op::<O>()` (the `OpRoutes` trait) adds `O::PATH` at
`O::METHOD` with one generic handler, `serve::<O>`: it extracts the path the
way the old handler did (`Path<PaneId>`, so a bad id gets axum's same
rejection), takes the JSON body with axum's `Json` (or none, when
`O::Req::without_body()` says there isn't one), checks the owner for an
`Access::Owner` operation (the handler's own check, as `owner_only` was),
runs the handler and answers with `Json`. In `api.rs` the route table keeps
its shape: `.op::<ClosePane>()` sits where `.route("/api/panes/{id}/close",
post(close))` was.

**The access check.** `authz::policy` asks `ops::policy(method, path)`
first, which matches each declared operation's template and turns its
`Access` into a `Policy`. A path that isn't an operation falls through to
the table as before. The table's special cases stay there: `drives` (typing
into a pane on the owner's machine needs their trust, M14) and `from_now`
(a share from now on reads nothing before it began, M13). Each could become
a flag on the declaration later (`DRIVES: bool`); the pilot doesn't need
them.

**The MCP tool.** `mcp::ops::def::<O>()` is the tool's row in the list
(`all_defs`), in the same place the hand-written `Def` was, so the list's
order doesn't move. Whether a read-only token may call it follows from the
method: only a GET reads, unless the declaration says `CREDENTIAL` (see
[Required role](#required-role-across-surfaces)). A kind of a grouped tool is `ops::kind::<O>()`,
a `const fn`, so it sits in the `LIST`/`SHOW` table as a row like the
others; the grouped tool itself (its lead sentence, `grouped_schema`, the
default kind) stays hand-written. A call goes through `ops::call::<O>`:
parse the agent's arguments, turn them into the operation's path and
request (`McpOp::args`: `"%7"` becomes 7), run the handler, shape the answer
(`McpOp::answer`, which adds the `summary`).

The agent's arguments are a type of their own on purpose. A pane is `7` or
`"%7"`, there's no path, defaults come from the caller's own pane, and the
schema's descriptions are written for agents. Deriving the schema from the
HTTP request would change every schema in the golden fixture.

**The CLI.** The CLI calls an operation instead of spelling it:

```rust
call::<ClosePane>(&sock, &p.0, &Empty {})?;               // was request(&sock, "POST", &format!(…), None)?.parse::<Empty>()?
let (env, v) = call_raw::<ShellEnvGet>(&sock, &(), &Empty {})?;   // for --json
let v = send_op::<ListPanes>(&sock, &(), &Empty {})?.json()?;     // read as it likes
```

The clap subcommand is **not** derived from the declaration, and the note
recommends it stays that way. Of today's 63 top-level commands, none is one
operation with the same arguments: each takes forms the API doesn't
(`%N`, `-` for stdin, a default pane from `$ARUGULA_PANE`), renders its own
summary, or spans several operations (`rules` is three routes, `shell-env`
two, `close` reaches a remote pane's own host first, `run --home` and
`agent` compose several calls). Deriving clap from the request type would
rename flags and change help, which is the drift the pilot must not cause.
What the CLI gets is the method, the path and the types from one place.

## Choosing the mechanism

The ticket names three ways to turn a declaration into surfaces. Measured
against this codebase:

| | Declarative macro (`macro_rules!` taking a block per operation) | **Trait plus a registry** (recommended) | Build-time generation (a spec file read by `build.rs`, or a proc macro) |
|---|---|---|---|
| Compile-time cost | Low; expansion is cheap | Low; no new crates, generic functions instantiated per operation | `build.rs` reruns on every spec change and adds a step to every build; a proc macro is a new crate on the critical path |
| How errors read | Poorly: a type error inside an expansion points at the macro, and a missing comma is "no rules expected the token" | Like any Rust: "the trait `Handle` is not implemented for `ClosePane`", at the impl | Errors in generated code point at `OUT_DIR` files; a spec error is whatever the generator prints |
| ts-rs and schemars | Fine, as long as the macro only names types | Unchanged: the types keep their derives, `Op` only names them | A spec duplicates the types or generates them, which fights ts-rs (it already generates TypeScript from Rust) and loses doc comments that schemars and ts-rs read |
| A grouped MCP tool | The macro has to grow a syntax for "kind of a tool" and emit table rows | A `Surface::Kind` const and a `const fn kind::<O>()` row in the existing table | The generator has to know `grouped_schema`'s rules, or the grouped tools stay outside it |
| Where an operation's code is | In the macro call: handler bodies inside a macro lose rustfmt and most IDE help | In ordinary `impl` blocks, one file per operation | In a spec and a handler file, kept in step by hand |

**Recommendation: a trait plus a registry.** `Op` in proto, `Handle` and
`McpOp` in the daemon, and one list of every operation (`every_op!` in
`ops/mod.rs`, a five-line `macro_rules!` that only names the types) for the
access check to walk. The macro is not the mechanism; it saves writing
the list twice. A test can check that every route in `api.rs` is either an
operation or on a short list of hand-written ones, so nothing slips past the
registry.

## What doesn't fit, and what it does instead

By name. "Hand-written" means it stays as it is today, beside the
operations, and may call an operation's handler.

- **Streaming:** `GET /api/panes/N/tail` with `follow`, `GET /api/events`
  (NDJSON), `GET /api/fs/watch`. The answer is a stream, not a value.
  Hand-written routes.
- **Binary and plain text:** `GET /api/panes/N/capture` (text),
  `GET /api/panes/N/export.cast` (asciicast), `GET /api/editors/vsix`,
  `POST /api/panes/N/upload` (raw bytes in, with its own body limit), the
  history pushes `POST /api/sync/N/{log,index,closed}` (raw bytes, a host
  token). Hand-written routes.
- **Block calls:** `POST /api/blocks/N/call/METHOD`. The arguments depend on
  the block's type and the method, and `authz.rs` decides per method
  (`enter`, `login`, `checkout` … are the owner's). These stay one
  hand-written route. Declaring each block type's methods belongs with the
  `Block` registration design (#459), not here. MCP's `draft` kinds and
  `agent_respond`, which are block calls, stay hand-written too.
- **Composed MCP tools:** `run` (with `wait`), `prompt_agent`, `start_agent`,
  `send_input` (text and keys), `attach`, `wait` (progress every 15 s and
  "still running" by 100 s), `read_output` (paging over the log). They stay
  hand-written tools. Once the operations under them exist they call
  `O::handle(&cx, …)` instead of reaching the mux themselves, which is where
  the second copy goes away.
- **Composed CLI commands:** `run --home`, `agent`, `close` (a remote pane's
  host first), `send --wait`, `rules`. They stay hand-written commands, each
  making several `call::<O>`.
- **Grouped MCP tools:** `list`, `show` and `draft` themselves stay
  hand-written: their lead text, the merged schema and the default kind.
  A kind that is one operation is declared (`list` kind `panes` in the
  pilot; `conversations`, `fountain_agents` and `show` kind `conversation`
  could be). Kinds that aren't operations (`list` kind `devices`, every
  `draft` kind, the `show` kinds that open a block with extra work after)
  stay hand-written rows.
- **Not HTTP at all:** the WebSocket (`/ws`, the share viewer's), the
  dial-out and provider tunnels (`/h/NAME/…`, `/tunnel/…`), `/mcp` itself.

## Required role across surfaces

The declaration's `ACCESS` is the HTTP rule, and it is what the pilot
derives the other checks from:

| `ACCESS` | HTTP (`authz.rs`) | Handler | MCP `Read` token | MCP agent block (`Block`) |
|---|---|---|---|---|
| `Owner` | only the owner | owner check (`serve`) | GETs only, not `CREDENTIAL` | allowed; the operation confines itself (the panes list shows only the block's tab) |
| `Pane(Viewer)` | that role on the pane's session | | GETs only, not `CREDENTIAL` | `readable`: the pane is in the block's tab |
| `Pane(Editor)` | that role | | GETs only, not `CREDENTIAL` | `drivable`: the block started it |
| `Handler` | the handler checks | the handler checks | GETs only, not `CREDENTIAL` | the handler checks |

**Read-only means it can't change anything or gain access.** A GET is
read-only by default, but a few GETs answer with something that grants
more: `GET /api/signin-link` returns a link carrying the daemon's local
token, and `/api/mcp/tokens` and `/api/shares` list secrets. Those declare
`CREDENTIAL: true`, and a read-only token can't call them. A test lists
every GET operation a read-only token can reach, against a checked-in
file, so a new one shows up in review.

**`authz.rs`'s special cases:**

- `drives` (typing into a pane on the owner's machine needs their trust,
  M14) becomes `DRIVES: true` on `send`, `keys`, `mouse`, `followup`,
  `upload` and `paste` as they convert. Today it matches path suffixes, so
  a new route ending in `/send` gets it by accident and a renamed one loses
  it; the flag ends that.
- `from_now` (a share from now on reads nothing before it began, M13)
  applies only to `capture`, `tail` and `export.cast`, which stay
  hand-written ([What doesn't fit](#what-doesnt-fit-and-what-it-does-instead)),
  so it stays in `authz.rs` beside them.
- The per-method block-call rules stay with block calls (#459).

So the guest-access rules stay reviewable in one place, a test writes every
operation's `ACCESS`, `DRIVES` and `CREDENTIAL` to a checked-in table, as the
golden MCP tool list does for tools.

## Labs

`Op::LABS` says an operation is listed only where the machine has the `labs`
file. For an MCP tool it replaces the operation's line in `UNLISTED` or
`UNLISTED_THREADS` (the tool list filters on it). The CLI keeps its own
`LABS_COMMANDS` list, because a command isn't an operation. When the
`labs` cargo feature arrives (#452), an operation behind it is
`#[cfg(feature = "labs")]` on its declaration, its impls and its line in
`every_op!`. None of the pilot's three is labs, so the pilot declares the
const and doesn't exercise it.

## Help text

MCP's descriptions and the CLI's help stay separate texts, because they
answer different readers. Two real examples:

- **close.** The CLI says "Close a pane (ending what runs in it); its
  output stays in history." MCP says "Close a pane or block, ending what
  runs in it." The person is told what they keep; the agent is told the
  tool takes blocks too.
- **wait.** The CLI says "Wait for a command, an exit, a match, or an
  agent. … Exits with the command's exit code; 124 on timeout." MCP says
  "… Answers "still running" with the offset after timeout seconds (default
  100): call it again." Exit codes mean nothing to an agent, and calling
  again means nothing to a shell script.

So neither is derived from the other. Each is in its surface's half: the
MCP description in `McpOp::SURFACE` (the operation's file in `ops/`), the
CLI's on its clap command. The operation's doc comment in proto describes
the route. Argument descriptions follow the same split: MCP's come from the
`Args` struct's doc comments (schemars), the CLI's from its clap struct.

## outputSchema

Now that each tool's answer is a type (#448), an `McpOp` could name it
(`type Answer: Serialize + JsonSchema`) and the tool list could carry its
`outputSchema`. The pilot keeps `answer` returning JSON, so this stays an
option; it needs `JsonSchema` on the result types in `mcp/results.rs`.

## If a surface is cut

"Fewer ways in" is for the team to decide (#213 holds it). This design
doesn't depend on the answer:

- **A CLI command dropped:** delete its clap variant and its `cmd/` file.
  The operation, its route and its MCP tool don't change. Other CLI
  commands that call it keep calling it.
- **An MCP tool dropped:** delete its `impl McpOp` and its row in
  `all_defs`; the golden fixture changes by that tool. The route doesn't.
- **A route nobody but MCP uses:** keep it; the route costs one line
  (`.op::<O>()`) and the web or a script may want it. If that ever matters,
  `PATH` becomes optional.
- **The TUI and tmux -CC** use the WebSocket and a few routes through the
  same CLI helpers; they need no change.

## The pilot

Branch `arch/451-ops-pilot`, three operations of different shapes:

1. **A simple read:** `shell-env`, which is two routes
   (`GET /api/hosts/self/shell-env`, `POST …/refresh`), no MCP tool, and one
   CLI command with a flag choosing between them.
2. **A write with a role check:** `close`. HTTP: an editor on the pane's
   session, and an invite block refuses a guest or an agent. MCP: the
   `close` tool, where an agent block may close only what it started. CLI:
   `arugula close`, which also closes a remote pane on its own host.
3. **The awkward one:** the panes list. HTTP `GET /api/panes` answers every
   `PaneSummary` to the owner; MCP's `list` is a grouped tool and this is
   its default kind, `panes`, which confines an agent block to its tab,
   marks the caller's own pane and answers `PaneEntry` rows with a summary;
   the CLI's `ls` reads untyped JSON in the pilot. Older daemons aren't
   supported ([Decided](#decided)), so ticket 1 makes it typed.

What it showed:

- The wire bytes, the MCP tool list and the CLI's output are unchanged:
  the golden test `the_tool_list_and_each_schema_are_unchanged` passes
  without re-blessing, nextest's counts match the base plus the pilot's
  three new tests, and `arugula ls|close|shell-env --help` and a real
  `ls`, `shell-env` and `close` against a daemon print the same bytes on
  the base and the pilot.
- `close` is where the role differences meet, and they fit in two
  questions to `Cx` (`may`, `agent_or_guest`) and one error type. MCP's
  invite check now goes through `is_invite`, as HTTP's does, instead of the
  pane summary's kind; the answer is the same.
- The grouped kind bends the design in the expected place: the operation
  can be a kind, but the grouped tool around it stays hand-written, and
  most of the kind's code is the answer's shaping (the tab, `you`, the
  summary), which is MCP's own and moves into `McpOp::answer` unchanged.
- The mechanism is about 560 lines once (proto's `op.rs`, `ops/mod.rs`,
  `mcp/ops.rs`, the CLI's `call`), and each operation then costs about what
  its handler did, minus the route line, the `Def` literal, the dispatch
  arm and the CLI's path string. The pilot is 774 lines added and 159
  deleted; most of the added lines are the mechanism and its tests.

## Migration

New operations first, then old ones when someone touches them anyway. No
big-bang conversion: a declared operation and a hand-written route sit side
by side, and `authz.rs`'s table only shrinks.

**How many would convert.** Method: each `(method, path)` pair in
`api.rs`'s router, read against its handler; each MCP tool and kind against
what it calls; each CLI request site by its route.

- **HTTP, `api.rs`:** 68 method-routes. 48 convert with what the pilot
  built (no path or a pane in the path; no body or a JSON body; a JSON
  answer). 13 more need two small additions: other path types (`usize`
  for `/api/rules/N`, `ThreadTarget` for `/api/threads/…`, `SessionId`,
  a name for `/api/studio/followers/APP` and `/api/agents/adapters/KIND`),
  and a query string as the request for a GET (`wait`, `history`,
  `search`, `conversations`, `fountain/agents`). 7 don't fit: `capture`,
  `tail`, `export.cast`, `events`, `vsix`, `upload`, and block `call`.
  So about 61 of 68.
- **HTTP, other modules** (`fs`, `hosts`, `share`, `labs/guest_ssh`, `acl`,
  `invite`, `setup`, `mcp/tokens`, `sync`, `resident`, about 40 more):
  most convert the same way; the sync pushes, `fs/watch`, the share viewer
  and the tunnels don't. Not counted route by route.
- **MCP:** 17 tools of their own plus 18 kinds of the three grouped tools,
  35 surfaces. 7 convert cleanly: `close`, `read_thread`, `post_thread`,
  `list` kinds `panes`, `conversations` and `fountain_agents`, `show` kind
  `conversation`. The rest are compositions, block calls or MCP-only
  (`device_call`, `invite_person`, `read_invite`, paging in `read_output`).
  They stay hand-written and start calling the operations' handlers as
  those appear. That's expected: the tools were shaped for agents rather
  than to mirror the API (#349).
- **CLI:** 128 request sites before the pilot. Every one whose route is an
  operation becomes a `call::<O>` (mechanical); the ones that stay are the
  streams, binary answers and block calls (about 15). No clap subcommand
  is derived (see above).

## Proposed tickets

Each is one PR. Filed on 2026-10-07 as #572–#580, in this order.

1. (#572) **Operations: the mechanism, with shell-env, close and the panes list.**
   The pilot, reviewed and tidied: `arugula_proto::op`, `daemon/src/ops/`,
   `mcp/ops.rs`, the CLI's `call`. Neutral names (`panes.list`,
   `pane.close`, `shell_env.get`, `shell_env.refresh`); `CREDENTIAL`, and
   the test listing what a read-only token may call; the checked-in access
   table; `ls` parses typed answers. Adds the `DECISIONS.md` entry and the
   test that every `api.rs` route is an operation or on the hand-written
   list. Depends on nothing.
2. (#573) **Operations: other path types and GET queries.** `PathArgs` for
   `usize`, `SessionId`, `ThreadTarget` and names; a GET's `Req` as its
   query, read with axum's `Query`, and the CLI building the same query
   string it does today. Converts `rules` (three routes) and `wait` as the
   proof. After 1.
3. (#574) **Operations: the pane verbs.** `send`, `keys`, `mouse`, `attention`,
   `followup`, `process`, `detection`, `diff`, `drivers`, and the CLI sites
   that call them; `DRIVES` as a declared flag replacing `authz::drives`.
   After 1.
4. (#575) **Operations: the owner's settings routes.** `ide`, `agents`, `studio`,
   `machines`, `push`, `notify`, `adapters`, `conversations`. Mechanical.
   After 2.
5. (#576) **Operations: the long waits.** `ask`, `permit`, `inbox`, `prompt`;
   MCP's `prompt_agent` calls `Prompt::handle` instead of `api::prompt`.
   After 3.
6. (#577) **MCP: the tools that are operations.** `read_thread` and `post_thread`
   (with `LABS` replacing `UNLISTED_THREADS`), `list` kinds `conversations`
   and `fountain_agents`, `show` kind `conversation`. Golden fixture
   unchanged. After 2 and 4.
7. (#578) **Operations: the other modules.** `fs` (not `watch`), `hosts`,
   `shares`, `guests`, `acl`, `invite`, `setup`, `mcp/tokens`, `synced`,
   `sandboxes`. May split by module. After 2.
8. (#579) **Optional: the operations table for the web client.** Emit each
   operation's method and path into `proto.gen.ts` beside its types, so
   the web client's fetches name the operation too. After 1.
9. (#580) **Optional: `outputSchema`.** `JsonSchema` on `mcp/results.rs`'s types
   and an `Answer` type on `McpOp`, listed as each tool's `outputSchema`.
   Changes the golden fixture on purpose. After 1.

## Decided

The owner answered the open questions on 2026-10-07:

1. **MCP descriptions** live in each operation's file, beside its handler;
   the tool list's order stays in `tools.rs`.
2. **Older daemons aren't supported.** The CLI parses typed answers from
   every operation, `ls` included, and doesn't keep untyped reads for
   `--host` to an older daemon.
3. **Read-only follows the method:** a GET is read-only unless it declares
   `CREDENTIAL` (its answer grants access, like the sign-in link). A test
   lists what a read-only token can reach.
4. **`drives` moves onto the declaration** as `DRIVES`; `from_now` stays in
   `authz.rs` with the hand-written streaming routes it applies to; a
   checked-in table of every operation's access keeps the rules reviewable
   in one place.
5. **`Op::NAME` is neutral,** `noun.verb`, not a surface's name.
