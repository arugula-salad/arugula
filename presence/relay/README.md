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
| `DEMO_SECRET` | the demo app's secret, so a restart keeps demo tokens good |
| `DATABASE_URL` | Postgres (`fly mpg attach` sets it). Without it in prod the relay runs without the Repo (`database: false`) and migrations do nothing. Dev and test default to a local Postgres on `127.0.0.1:55432` (`docker run -p 127.0.0.1:55432:5432 -e POSTGRES_PASSWORD=postgres postgres:17`); `TEST_DATABASE_URL` overrides the test one |
| `ECTO_IPV6=true` | on Fly: the private network, and the Managed Postgres host on it, is IPv6 only |

`GET /healthz` says `ok` (the machine check). `GET /healthz?db=1` also asks
Postgres: `ok db`, `ok no-db` without one, or 503 `db down`. Anything else
not a file in `priv/static` is the demo site's index (step 3 builds the
site there).

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

## Members by sign-in (no server of the app's)

An app can name its sign-in provider (Clerk, Auth0, Firebase, Supabase or
any OIDC issuer: `PUT /api/v1/apps/<id>/issuer`), and then a page with its
publishable key joins with the person's own token as `auth`
(PROTOCOL.md, "Members by sign-in"; `PresenceRelay.Issuers`). The
providers' keys are fetched and cached per node (`PresenceRelay.Issuers.Keys`).

To check a real provider's token against an issuer, with no database or
relay running (the token from stdin or `AUTH_TOKEN`, never argv):

```sh
pbpaste | mise exec -- mix run --no-start scripts/check_provider.exs -- \
  --provider clerk --issuer https://your-app.clerk.accounts.dev --origin http://localhost:3000
```

It prints the issuer it built, the token's header and the claims it
checks, then `admitted` (with the member's sub and name) or `refused` and
why. `--audience`, `--project`, `--name-claim`, `--room-claim`/`--room-match`
and `--room` set the rest.

## Usage and caps

Each app in the database is metered (#318's P9; PROTOCOL.md, "Usage and
caps"; `PresenceRelay.Usage`): monthly active users and call minutes,
rolled up by day.

| table | |
|---|---|
| `usage_days` | `(app_id, day)`: `active_users` that day, `call_seconds` (each person's time in each call, rounded up to a second) |
| `usage_members` | `(app_id, month, member_key)`: each person counted that month, once; `days` has bit d-1 set for each day d they joined on |
| `apps.mau_cap` | the cap on monthly active users, null for none |

Each node buffers joins and call time and writes them every 5 s, one
transaction per app and day (`INSERT … ON CONFLICT` on the members, then
the day's counters go up by the people new that day). So every count is
exact across nodes and restarts; a node that crashes loses up to 5 s of
call time (its joins come back on the next join). Caps are checked at
join against counts each node caches: no query on a join until the app
nears its cap, and a cluster may overshoot by the new people other nodes
let in within about one flush. The load test runs two nodes over three
apps and three days and checks every number:
`mix test test/presence_relay/usage_load_test.exs`.

## Deploying

`relay/Dockerfile` builds the demo, the SDK and the hosted installs, then
the release; it's built from `presence/`. Two Fly apps, both personal org:

- **staging**, `arugula-stream-staging` (`presence/fly.staging.toml`, iad,
  one machine): `cd presence && fly deploy -c fly.staging.toml --ha=false`.
  Postgres: the Managed Postgres cluster `arugula-stream-db`
  (`nlkxjo5pdkd0y93v`, iad), database and user `presence_staging`,
  attached as `DATABASE_URL`; the `release_command` migrates before each
  release. https://arugula-stream-staging.fly.dev
- **production**, `arugula-stream` (`presence/fly.toml`): arugula.stream.

### Production: attach Postgres and move to iad (when Jake says)

Production has no database yet and runs in ewr; the cluster is in iad
(Managed Postgres isn't offered in ewr). From `presence/`:

1. A database and user of its own on the cluster:
   `fly mpg databases create nlkxjo5pdkd0y93v --name presence_prod` and
   `fly mpg users create nlkxjo5pdkd0y93v --username presence_prod --role schema_admin`.
2. Attach it: `fly mpg attach nlkxjo5pdkd0y93v -a arugula-stream --database presence_prod --username presence_prod`
   (stages `DATABASE_URL`; nothing restarts yet).
3. In `presence/fly.toml`: `primary_region = "iad"`, `ECTO_IPV6 = "true"`
   under `[env]`, and the staging file's `[deploy]` block
   (`release_command = "/app/bin/presence_relay eval PresenceRelay.Release.migrate()"`).
4. Deploy: `fly deploy --ha=false --remote-only`. Fly starts the new
   machine in iad from `primary_region`; check `fly status -a arugula-stream`
   shows iad, and if an ewr machine is left, `fly machine destroy <id> -a arugula-stream`.
   The DNS records (A and AAAA to the app's shared IPs) don't change with
   the region.
5. Check: `curl https://arugula.stream/healthz?db=1` says `ok db`, and the
   site and a demo room still work.

Rollback: `fly secrets unset DATABASE_URL -a arugula-stream` and redeploy:
the relay runs without the Repo again.

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
