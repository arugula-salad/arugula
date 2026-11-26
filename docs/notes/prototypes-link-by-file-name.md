---
section: Fixed
title: A Doc's prototypes can link to each other
line: A prototype that links to another prototype of its Doc by file name now shows it in place, instead of stopping with "tried to leave the page".
headline: no
---
A link or `location.assign` to `next-screen.html` shows the prototype titled "Next screen" in the same frame. Anything else a prototype goes to still stops it. Prototypes shown from a share link switch on a link click; a write to `location` there still stops it.
