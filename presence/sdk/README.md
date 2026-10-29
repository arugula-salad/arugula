# arugula.stream server SDKs

Your server signs a **room token** for each signed-in user: which room
they may join and what to call them (PROTOCOL.md, "Room token"). Your
page's `<arugula-room token-url="…">` (or `connect({token})`) fetches it
from an endpoint you write. Same call in every language, no dependencies
beyond the standard library, and byte-identical tokens for the same inputs
(`vectors.json`, which every SDK's tests and the relay's check).

| Language | Where | The call |
|---|---|---|
| Node / TypeScript | [`node/`](node/) | `createRoomToken({ app, secret, room, user: { id, name } })` |
| Python | [`python/`](python/) | `create_room_token(app=, secret=, room=, user_id=, name=)` |
| Go | [`go/`](go/) | `presence.CreateRoomToken(presence.TokenOptions{App, Secret, Room, UserID, Name})` |
| Elixir | [`elixir/`](elixir/) | `ArugulaPresence.create_room_token(app:, secret:, room:, user_id:, name:)` |
| Ruby | [`ruby/`](ruby/) | `ArugulaPresence.create_room_token(app:, secret:, room:, user_id:, name:)` |

All take an optional `look` (`{character, colour}`, each 0–255) and `ttl`
(seconds, default 3600, at most 86400). Room and app ids are 1–128 of
`A–Z a–z 0–9 _ . : -`, user ids also allow `@`, names are 1–80 characters
with no control characters. Each README has a token endpoint for that
language's usual web framework.

The token is an HS256 JWT: header exactly `{"alg":"HS256","typ":"JWT"}`,
claims compact in the order `app, room, sub, name, exp` then `look`,
base64url without padding (API.md).
