---
section: Fixed
title: A machine refuses a token from before it started
line: A machine refuses a token issued before it started, so a token can't be taken again after a restart; the page and the CLI ask control for a new one.
headline: no
---
A machine remembers the tokens it has taken only while it runs. It now also
refuses any token issued before it started, so one taken before a restart
isn't taken again after it. The page and `arugula` ask control for a new token
when a machine refuses one, so right after a restart that costs a second or
two at most.
