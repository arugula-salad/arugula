---
section: Fixed
title: link takes a list for to
line: An agent's link tool now accepts `to` as a list (from X, to [A, B]) and links each one, answering a line per link, as its schema says.
headline: no
---
`link` already took up to 50 `add` or `remove` entries; a `to` written as a list
was refused as unreadable. It is now one link for each item, each landing on its own.
