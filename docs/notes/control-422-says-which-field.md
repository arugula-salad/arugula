---
section: Fixed
title: A refused request says which field and why
line: When Arugula cloud refuses a tool's request as malformed, the tool now says which field and why, not just "control answered 422".
headline: no
---
When Arugula cloud answers a tool's request with a 400 or 422 and a plain-text reason, the tool now passes the reason on. A `watch create` with `standing` or `exclude_self` against a cloud that doesn't keep them yet says "Arugula cloud doesn't keep standing watches or exclude_self yet: make a watch without them", where it used to say only "control answered 422".
