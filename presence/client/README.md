# @arugula/presence

Join a room and see who's around as live avatars or outlines (arugula.video,
Arugula #315). A `PresenceClient` (`src/client.ts`) holds your camera (outline
or avatar mode, your character and colour, the outline settings) and
everyone else's newest frame, for any view to subscribe to and draw with
`@arugula/presence-core` (`../core`). It talks to people through a
`Transport` (`src/transport.ts`):

- `phoenixTransport` (`src/phoenix.ts`): arugula.video's Phoenix relay, as
  `../PROTOCOL.md` says: a room token, the room's channel, binary frames,
  Phoenix.Presence for members, waves.
- `arugulaTransport` (`src/arugula.ts`): Arugula control's relay (POSTs and
  Server-Sent Events, team rosters on control's side).

Arugula imports `src/client.ts` and `src/arugula.ts` by path, so its bundle
doesn't carry Phoenix's client.

    pnpm install
    pnpm test        # node: the client over a fake transport, the relay's frame prefix, members
    pnpm typecheck
