---
schema: 1
id: "aru-138"
title: "Video presence is removed"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  harness: "claude-code"
question: "Does Arugula keep video presence: the People block (a team's arugula.stream room with avatars, voice and calls) and live presence (live faces on avatars, the `live-presence` Labs flag)?"
options:
  - id: "a"
    label: "Take it out of Arugula"
    how: "Remove the People block, live presence, control's room tokens and its arugula.stream CSP, and their docs; keep what a mixed-version fleet still sends readable."
    tradeoff: "Who's around stays as chat's presence dots only; bringing video back means reviving the code from git."
rejected: []
supersedes: []
evidence: []
choice:
  option: "a"
  reason: "Jake's call, 2026-11-09: \"i want to take the video call / video presence stuff out of the app\"."
decided_by: "jhgaylor"
decided_on: "2026-11-09"
reviews: []
constrains:
  - "path:crates/proto/src/flags.rs"
  - "path:crates/proto/src/lib.rs"
  - "path:crates/desktop/Info.plist"
---

# Video presence is removed

The People block (#1792: a team's room on arugula.stream, with avatars or
outlines from the camera, waves, the room's voice and calls through LiveKit)
and live presence (#341–#347: Off, Watching or Live, live faces on avatars
across the app, the `live-presence` Labs flag) were taken out of Arugula on
2026-11-09, Jake's decision. arugula.stream itself (its own repo, relay and
site) isn't Arugula's and is untouched. Revive the Arugula side from git if
it's wanted again: everything is in the tree before this change
(`crates/daemon/src/people_block.rs`,
`crates/platform-chat/src/chat/presence_room.rs`, `web/src/shared/live/`,
`web/src/shell/blocks/people.tsx`, and their tests).

Chat's presence (active, away, offline, #110) stays: it was never video.

What stays, for a fleet whose versions are mixed:
- **`live-presence` is a retired flag** (`flags::RETIRED`): a `flags.json`
  or a control push that names it loads, means nothing, and is kept by a
  write; setting it says why it's gone.
- **A stored People block is dropped as the daemon restores.** `people`
  parses as `BlockType::Unknown`, which no kind makes, so the leaf goes
  and the rest of the layout stays, as a remote block's did.
- **An older page's `live` in a presence beat is ignored**, and no answer
  says `live`, which an older page reads as the flag off.
- **`chat_presence.live` stays in the schema** (migration 0070), unread
  and unwritten, so control can still roll back past this.
- **The macOS app keeps the camera** (`NSCameraUsageDescription`, the
  camera entitlement): the QR scanner (`web/src/shell/ui/scan.tsx`) uses it.
  The microphone stays for a lent device's `record_audio`, as aru-067 has
  it.
Where: `crates/proto/src/flags.rs` (`RETIRED`), `crates/proto/src/lib.rs`
(`BlockType`), `crates/platform-chat/src/chat_people_tests.rs`
(`an_older_pages_live_choice_is_ignored`), `crates/desktop/Info.plist`.
From: Jake, 2026-11-09.
