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

## The demo (arugula.video)

`presence/demo` builds into `priv/static` (git ignores it): `cd ../demo &&
pnpm install && pnpm assets && pnpm build`, then `DEMO=1 mix phx.server`.
`BIND=<address>` listens somewhere other than loopback in dev (a tailnet
address, for a browser on another machine; that browser needs a secure
context for the camera: https, or Chrome's
`--unsafely-treat-insecure-origin-as-secure=http://<address>:<port>`).

## A cluster (two or more machines)

The relay clusters (#318's P2): Phoenix.PubSub and Presence span the nodes,
connections are an OTP `:pg` group per member (so the room and connection
caps count the whole cluster, and a camera goes `off` when a member's last
connection anywhere leaves), and each node keeps the newest frames sent
through it, which a joiner on any node is sent (`PresenceRelay.Rooms`).

On Fly:

1. Give the nodes a shared cookie, once per app:
   `fly secrets set RELEASE_COOKIE=$(openssl rand -hex 32) -a arugula-stream`
2. Deploy as usual. `rel/env.sh.eex` names each machine's node after the
   app and its private IPv6 address and sets
   `DNS_CLUSTER_QUERY=<app>.internal`; `DNSCluster` finds the others.
3. Scale out: `fly scale count 2 -a arugula-stream` (or one machine per
   region: `fly scale count 2 --region iad,ord`). Check from a machine with
   `fly ssh console -C "/app/bin/presence_relay rpc 'IO.inspect(Node.list())'"`.

Locally, two nodes on one machine:

    DEMO=1 DEMO_SECRET=s PORT=4202 elixir --sname a --cookie c -S mix phx.server
    DEMO=1 DEMO_SECRET=s PORT=4212 elixir --sname b --cookie c -e 'Node.connect(:"a@HOST")' -S mix phx.server

(The same `DEMO_SECRET` on both, so a token from one node joins on the
other.) The multi-node tests start a second node with OTP's `:peer`:
`mix test test/presence_relay/cluster_test.exs`.
