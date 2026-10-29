# arugula-platform-chat

Chat and agent sessions (#314), on the platform's base
(`arugula-platform-base`, [crates/platform](../platform/README.md)). They're
one crate because they're one cycle: a session is posted, carded and
answered in its thread, and a thread's `@arugula` starts one.

- `src/chat.rs` and `src/chat/`: a team's channels and DMs, and the threads
  on panes, sessions and objects (M122, M138, M196): who sees a room, its
  messages, pushes, search, files, previews, pins, reactions, saved
  messages, typing, the live stream's route, and agents' threads (M197,
  M198).
- `src/sessions/`, `src/session_live.rs`: the agent sessions in those
  threads (M256 onwards): starting one with `@arugula`, its job, its card
  and page, its PR (M259), CI and review feedback (M260), Try 3 (M264),
  merges (M265), and a hosted box's session calls.
- The tests that drive chat's routes, whatever they test (issues' threads,
  the inbox's mentions, notification levels, the hub calls a page of rooms
  makes, a board's thread): the base can't serve chat's routes, so they're
  here.

Chat's core stays in the base (`arugula_platform::chat`): the live bus on
`Platform::chat` (open streams, `chat_events`, typing, session moves),
whether chat is on for an account, names and mentions, an account's
erasure, and following the hub's change feed. So issues, boards, the inbox,
the cache and the hub's port tell a thread something without this crate.
Its tables (`db/chat*.rs`, `db/session*.rs`) stay in the base too: the
forge, hosted workspaces, the inbox and runs read them.

The rest of the platform reaches this crate through `features::Chat`: the
hub's port for a session's PR and a hosted box's session calls, the forge
mirror and GitHub's webhook for what a session hears, hosted workspaces
for a rebuilt box's sessions, boxes for a review, grants for the notice a
room gets, and agents with no machine (`arugula-platform-agents`) for
their threads, so agents doesn't stack on chat. The crate adds its routes,
the sessions' job and saved messages' reminders through
`features::Feature`. Its tests: `scripts/server-cargo nextest run -p
arugula-platform-chat`; their `crate::testing` and `crate::chat_tests`
register the crate before the base's harness makes a router.
