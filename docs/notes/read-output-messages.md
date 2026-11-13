---
section: Added
title: read_output reads an agent's messages
line: read_output on an agent block takes format messages: just the prompts, its text and a line per tool call, paged by message, and a running agent's last_lines read the same way.
---
`read_output` with `format: "messages"` returns only what was said, with no tool payloads or results, about 16k characters a page, and `next_offset` to go on. Terminal panes keep their raw output.
