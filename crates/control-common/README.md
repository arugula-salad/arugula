# arugula-control-common

What Arugula control's two processes both use ("Control is two processes:
a hub and a platform", #1150): the hub (`arugula-control`) and the platform
(`arugula-platform`).

- `sqlite`: opening the database. WAL, foreign keys and a busy timeout, so
  the platform's second connection to the hub's file (in process) waits
  for a lock rather than failing.
- `error`: `ApiError`, `err`, `refusal` and `reply`, the JSON both answer
  with.
- `limit`: rate limits, counted in memory. Each process keeps its own, and
  each limit is counted by one process only.
- `analytics`: the PostHog client. In process the platform shares the hub's
  queue; on its own it starts one with the same key.
- `principal`: who is asking (`Principal`), and the HMAC-signed header the
  hub's gateway forwards it in once the platform is its own process
  (M140): a `Vouch`, which is a principal, a refusal or nobody.
- `internal`: how the hub's and the platform's calls to each other are
  signed with the same key (M140), each call good once.
- `roots`: outgoing TLS roots, the system's or the bundled ones.

It knows nothing of either side's state: no `App`, no tables.
