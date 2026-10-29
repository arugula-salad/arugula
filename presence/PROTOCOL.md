# arugula.video presence protocol, v0

How a page shows who's around as live avatars or outlines, and how the
relay passes it on. The relay sees opaque frames and who's in a room;
no picture of anyone ever reaches it. Grew out of Arugula's S43 (outlines)
and S44 (rigged avatars) spikes; tracker Arugula #315.

## Pieces

- **Host app server**: signs a **room token** for each of its users with
  the app's secret. It alone decides who may be in which room.
- **Client** (`@arugula/presence`, `presence/client`): joins a room with a
  token, publishes the user's frames and state, receives everyone else's.
- **Relay** (`presence/relay`, Phoenix): rooms, membership, fan-out. It
  keeps each member's newest frame in memory for a joiner, and nothing
  else.

## Connecting

A Phoenix socket (serializer V2, JSON with binary pushes) at
`/socket/websocket?vsn=2.0.0&<params>`, where the params are one of:

| params | who decides who's in the room |
|---|---|
| `token=<room token>` | the host app's server, which signed it (below) |
| `key=<publishable key>&room=<room>&auth=<provider token>[&name=<name>][&look=…]` | the app's sign-in provider: a **member** (below) |
| `key=<publishable key>&room=<room>&name=<name>[&look=<character>:<colour>]` | nobody: a **guest**, where the app allows guests |

The socket refuses a missing, expired or badly signed token, a
publishable key that's unknown, revoked, or not from one of its app's
allowed origins, and a provider token the app's issuer doesn't vouch
for (a bad `auth` is refused, never let in as a guest). Either way the
room channel is the same.

### Publishable keys

A hosted relay (arugula.stream) keeps apps in its database, each with
publishable keys (`pk_live_…`, `pk_test_…`), safe to put in a page:

- The socket's `Origin` header must match one of the app's allowed
  origins (`scheme://host[:port]`; `https://*.example.com` matches its
  subdomains, not the bare domain). A `pk_test_` key is also accepted
  from `localhost`, `127.0.0.1` and `[::1]` on any port. No `Origin`, no
  key: a key is for browsers.
- A guest gives only a `name` (rules as the token's `name`); the relay
  makes their `sub`, `g_` and 12 hex digits, new for each socket.
- `room` follows the token's `room` rules; the channel topic must match it.
- Keys are revoked one at a time; the app's server secret (for room
  tokens) is shown once and rotated on its own.

### Members by sign-in

An app may name one **issuer**, its sign-in provider; then a page with
its key sends the person's own token from that provider as `auth`, and
the relay checks it against the provider's published keys. No server of
the app's is involved.

| provider | settings | the token's `iss` | keys |
|---|---|---|---|
| `clerk` | `issuer`: the Frontend API URL | that URL | `<iss>/.well-known/jwks.json` |
| `auth0` | `issuer`: the tenant's (or custom) domain | `https://<domain>/` | `<iss>.well-known/jwks.json` |
| `firebase` | `project`: the project id | `https://securetoken.google.com/<project>` | Google's securetoken JWKS |
| `supabase` | `issuer`: the project URL | `<url>/auth/v1` | `<iss>/.well-known/jwks.json` |
| `oidc` | `issuer`: exactly the tokens' `iss` | that | its `/.well-known/openid-configuration`'s `jwks_uri` (whose `issuer` must match) |

- **Signature**: RS256 (RSA ≥ 2048 bits), ES256 (P-256) or EdDSA
  (Ed25519) with the key the header's `kid` names; `none` and HS* are
  refused. A Supabase project still signing with its legacy HS256 secret
  isn't supported: it needs asymmetric signing keys.
- **Claims**: `iss` exactly; `aud` must hold the issuer's `audience`
  (required for Auth0 and OIDC; Firebase's is the project id;
  Supabase's `authenticated` by default; Clerk's session tokens carry
  none, so it's checked only when set); `exp` required, `exp` and `nbf`
  with 30 s of leeway; `sub` 1–128 printable characters. Clerk: an `azp`
  must equal the page's `Origin`, and `sts: "pending"` is refused.
  Firebase: `auth_time` not in the future.
- **Keys** are cached per URL for the response's `max-age` (held to
  60 s–24 h, an hour without one). A `kid` the cached set lacks fetches it
  once more, at most every 30 s per URL. Fetches time out after 5 s and
  only go to `https://` on public hosts.
- **Who**: the member's `sub` is `<provider>-<tag>:<their sub>`, the tag
  the first 8 hex digits of SHA-256 of `iss` (so two issuers' users never
  share one, and none starts `g_`). Their name is the issuer's
  `name_claim` (default `name`; Supabase's `user_metadata.full_name`; a
  dotted name walks into maps when no claim has the whole name), else
  the socket's `name`, else `Member`; at most 80 characters.
- **Room rule** (optional, one per issuer): `{"claim": c, "match": m}`.
  `prefix`: claim `c` (a string) equals the room's id before its first
  `:` (`org_id` = `acme` admits `acme:standup`, not `globex:standup` or
  `acme`). `contains`: claim `c` is a list holding the room's id (or a
  string equal to it). Nothing else is accepted. Guests aren't held to it.
- The key's origin rules apply as for guests. The token is checked when
  the socket connects; a reconnect sends a fresh one (the SDK asks for
  it).

Hosted, the owner sets it with `PUT /api/v1/apps/<id>/issuer`
(`{provider, issuer?, project?, audience?, name_claim?, room_rule?}`;
`GET` shows it with the `iss` and key URLs it makes, `DELETE` removes it).

### Room token

A JWT, HS256, signed with the app's secret (the relay has each app's
secret by its `app` id, from its config or, hosted, its database; a demo
relay has one app, `demo`):

| claim | |
|---|---|
| `app` | the app's id |
| `room` | the room's id within the app (`[A-Za-z0-9_.:-]{1,128}`) |
| `sub` | the user's id within the app (`[A-Za-z0-9_.:@-]{1,128}`) |
| `name` | what to call them (≤ 80 chars) |
| `exp` | expiry, Unix seconds (a token lives ≤ 24 h) |
| `look` | optional `{character, colour}` (bytes) to start with |

A **demo relay** (`DEMO=1`) also hands out tokens for app `demo` from
`POST /api/demo/token` with `{room, name}`: anyone may join any demo room
by its name, and `sub` is random.

## The room channel

Topic `room:<room>` (the app comes from the token or the key; a socket
for another room is refused). Join payload: `{}`. The reply is `{you:
<sub>, calls: bool}` (`calls`: whether this relay has calls; below).

### Presence

Phoenix.Presence on the topic: `presence_state` and `presence_diff` as
Phoenix sends them, keyed by `sub`, one meta per connection:

```json
{ "name": "Jake", "look": {"character": 2, "colour": 0},
  "mode": "avatar" | "outline" | "none", "away": false, "watching": true,
  "call": null | "<call id>" }
```

`mode` is what the member's camera sends (`none`: camera off). `watching`
is whether any of their pages is showing others (so a sender with nobody
watching can drop to a frame every few seconds). `call` is the call this
connection is in (the relay sets it; a client can't), for tiles.

### Client → relay

| event | payload | |
|---|---|---|
| `frame` | binary, ≤ 32 KiB | the member's newest frame (below). Frames closer than 70 ms to the member's last are dropped. |
| `state` | `{mode?, look?, away?, watching?}` | updates the member's presence meta |
| `wave` | `{to: <sub>}` | wave at someone: a hello (where the relay has calls, a page asks to talk with `call` instead) |
| `answer` | `{to: <sub>, yes: bool}` | answer a wave |
| `say` | `{text}` | a text message for the room: 1–2000 characters after trimming, no control characters but newlines; at most 5 per 5 s per connection. Replies `ok` with the kept message, or `error` `{reason: "empty" \| "too long" \| "control characters" \| "too fast"}` (and it goes nowhere) |
| `history` | `{before: <id>}` | the 50 messages before that one; replies `ok` `{messages: [...]}`, oldest first |
| `react` | `{emoji}` | an emoji for the room (a 👍, a 🎉): one grapheme, no ASCII, ≤ 32 bytes, at most one per 250 ms per connection, else dropped |
| `call`, `call_answer`, `call_leave`, `call_token` | | see "Calls" |

### Relay → client

| event | payload | |
|---|---|---|
| `frame` | binary: `len:u8 sub:utf8[len] frame` | someone's frame, to everyone else in the room; on join, every member's newest frame younger than 10 s |
| `off` | `{sub}` | someone's camera went off, or their frame went stale (10 s) |
| `wave` | `{from: <sub>}` | to the waved member only |
| `answer` | `{from: <sub>, yes}` | to the waver only |
| `said` | `{id, sub, name, text, at}` | a message, to everyone in the room but the connection that said it (which has it in its reply); `id` only goes up in a room, `at` is Unix ms |
| `history` | `{messages: [...]}` | on join: the room's last 50 messages, oldest first |
| `react` | `{from: <sub>, emoji}` | someone's reaction, to everyone else in the room (not stored: a joiner doesn't get old ones) |
| `call`, `call_cancel`, `call_answer`, `call_join` | | see "Calls" |

Limits: 32 KiB a frame, 70 ms between a member's frames, 64 members a room,
8 connections a member. Over a limit, the frame is dropped (no error);
too many members or connections, the join is refused (`{reason}`).

## Calls

A wave that's answered yes becomes a voice call (#318's P7), on LiveKit.
The relay rings, hears the answer, and hands each party a LiveKit access
token for one LiveKit room; audio goes between the browsers and LiveKit,
never through the relay. Audio only for now (video later).

A relay has calls when it has LiveKit's URL and an API key and secret
(`LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET`). Without them,
the join reply says `calls: false`, `call` is refused with "calls aren't
set up here", and nothing else changes: a page hides its call buttons and
waves stay waves.

### The flow

1. A asks B to talk: `call {to: B}`. The relay replies `ok {id}` (a new
   call's id, 16 url-safe characters) and pushes every one of B's
   connections `call {from: A, id}`.
2. Any one of B's connections answers: `call_answer {id, yes}` (replies
   `ok`). B's other connections get `call_cancel {id}`. A's calling
   connection gets `call_answer {id, from: B, yes}`.
3. On a yes, the relay pushes A's calling connection and B's answering one
   each `call_join {id, url, room, token}`: LiveKit's URL, the LiveKit
   room's name, and a token for that connection alone. Each page joins the
   LiveKit room with it. Both members' presence metas say `call: id`.
4. Either leaves with `call_leave {id}` (replies `ok`), or by closing the
   page; their meta goes back to `call: null`. A page alone in a call (the
   others left) leaves it too.

A ring nobody answers in 30 s ends: B's connections get `call_cancel {id}`
and A's gets `call_answer {id, from: B, yes: false, reason: "no answer"}`.
A gives up a ring before it's answered with `call_leave {id}` (B's get
`call_cancel`).

**Growing a call.** A connection that's in a call and rings someone rings
them into that call: the reply carries the call's id, and on a yes only
the new member gets a `call_join` (the others are already there). A call
can hold anyone in the room the people in it ring in.

### Client → relay

| event | payload | |
|---|---|---|
| `call` | `{to: <sub>}` | ring someone (into your call, if you're in one). Replies `ok` `{id}`, or `error` `{reason: "calls aren't set up here" \| "that's you" \| "not in the room" \| "already in the call" \| "too fast"}`. At most 3 rings per 30 s per connection |
| `call_answer` | `{id, yes: bool}` | answer a ring this connection was pushed. Replies `ok`, or `error` `{reason: "no such call"}` (never rung here, answered already, or over) |
| `call_leave` | `{id}` | leave the call, or give up ringing. Replies `ok` |
| `call_token` | `{id}` | a fresh `{id, url, room, token}` for the call this connection is in (after LiveKit dropped the page), or `error` `{reason: "not in that call"}` |

### Relay → client

| event | payload | |
|---|---|---|
| `call` | `{from: <sub>, id}` | someone rings you, to each of your connections |
| `call_cancel` | `{id}` | a ring you could answer is over: answered on another of your pages, given up, or unanswered |
| `call_answer` | `{id, from: <sub>, yes, reason?}` | the answer to your ring, to the connection that rang |
| `call_join` | `{id, url, room, token}` | join this LiveKit room: you're in the call |

### The LiveKit room and token

The LiveKit room is named `<app>:<room>:<call id>`, so two apps (or two
rooms) never share one. The token is LiveKit's access token
(docs.livekit.io, "Authentication"): a JWT, HS256, signed with the API
secret:

| claim | |
|---|---|
| `iss` | the API key |
| `sub` | the member's `sub` (their LiveKit identity) |
| `name` | the member's name |
| `nbf`, `exp` | now, and now + 10 minutes. LiveKit checks it on connecting only, and refreshes a connected page's token itself; a page that drops out asks `call_token` |
| `video` | `{room, roomJoin: true, canPublish: true, canPublishSources: ["microphone"], canPublishData: false, canSubscribe: true}`: this room only, the microphone only |

### Metering

The relay tells each call's life as telemetry events (`[:presence_relay,
:call, :start | :join | :leave | :stop]`, `:leave` with `duration_ms`),
all from `PresenceRelay.Calls`; nothing meters them yet.

## Frames

Opaque to the relay. Byte 0 says how the rest is encoded (`0` raw, `1`
deflate-raw of raw); raw byte 0 says what it is:

- `1`–`3`: an **outline** (presence-core's `frame.ts`): polylines in grid
  cells, v2 with a class byte per line, v3 with a nearness byte.
- `0x40`: a **rig** (presence-core's `rig.ts`): head, face, shoulders,
  wrists, hands, place in frame, and the sender's look. 21–40 bytes.

A client that doesn't know a kind or version skips the frame.
