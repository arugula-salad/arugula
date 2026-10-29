# presence/relay: arugula.video's relay

The relay for embeddable presence (Arugula #315): rooms, who's in them,
and each member's newest frame passed to the rest. It implements
[`../PROTOCOL.md`](../PROTOCOL.md). It never sees a picture of anyone:
frames are opaque outlines or rigs, kept in memory only as each member's
newest, for a joiner.

Phoenix (channels and `Phoenix.Presence`, no LiveView, no database) on
Bandit. Room tokens are HS256 JWTs checked with `:crypto`.

## Run it

```sh
mix deps.get
DEMO=1 mix phx.server            # http://localhost:4000, PORT=… to move it
mix test
```

The toolchain is pinned in `mise.toml` (Erlang 28.3, Elixir 1.19.2): Phoenix
needs an Erlang with public_key's headers, and the system one may have none.

## Environment

| variable | |
|---|---|
| `PRESENCE_APPS` | `id:secret,id2:secret2`: each host app's id and the secret its server signs room tokens with |
| `DEMO=1` | adds app `demo` (a random secret at boot) and `POST /api/demo/token` with `{room, name}` → `{token, room, sub}`: anyone may join any demo room |
| `PORT` | default 4000 |
| `PHX_HOST` | prod: the public host (default `arugula.video`) |
| `SECRET_KEY_BASE` | prod: `mix phx.gen.secret` |

`GET /healthz` says `ok`. Anything else not a file in `priv/static` is the
demo site's index (step 3 builds the site there).

## Minting room tokens (your server)

A token admits one user to one room of your app for at most a day. Sign it
with your app's secret; the relay only checks it.

Elixir (no library needed):

```elixir
defmodule MyApp.Presence do
  def token(user, room) do
    claims = %{app: "myapp", room: room, sub: user.id, name: user.name,
               exp: System.system_time(:second) + 3600}
    enc = &Base.url_encode64(Jason.encode!(&1), padding: false)
    data = enc.(%{alg: "HS256", typ: "JWT"}) <> "." <> enc.(claims)
    sig = :crypto.mac(:hmac, :sha256, System.fetch_env!("PRESENCE_SECRET"), data)
    data <> "." <> Base.url_encode64(sig, padding: false)
  end
end
```

Node (`jsonwebtoken`):

```js
import jwt from "jsonwebtoken";
export const presenceToken = (user, room) =>
  jwt.sign({ app: "myapp", room, sub: user.id, name: user.name }, process.env.PRESENCE_SECRET, {
    algorithm: "HS256",
    expiresIn: "1h",
  });
```

The page then connects with the `phoenix` client:

```js
import { Socket } from "phoenix";
const socket = new Socket("wss://arugula.video/socket", { params: { token } });
socket.connect();
const room = socket.channel(`room:${roomId}`, {});
room.on("frame", (buf) => { /* ArrayBuffer: len:u8, sub, frame */ });
room.join().receive("ok", ({ you }) => { /* … */ });
room.push("frame", frameBytes.buffer); // an ArrayBuffer
```

## Deploying

`Dockerfile` builds a release; `fly.toml` is for app `arugula-video`, not
created yet.
