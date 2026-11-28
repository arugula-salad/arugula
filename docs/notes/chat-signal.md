---
section: Added
title: A chat post can carry a signal to watch for
line: A post can carry a signal, like pr_ready, that a watch on chat.reply or chat.posted matches.
headline: no
---
`post_thread` takes an optional `signal`: a short slug (lowercase letters, digits, `_` and `-`, starting with a letter, at most 40 characters) such as `pr_ready` or `blocked`. A lead can sleep until a worker posts it with `watch` event `chat.reply`, match `{room, signal}`, instead of waking at every `agent.idle`. Board leads and issue agents are told to use it.
