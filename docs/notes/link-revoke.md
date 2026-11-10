---
section: Added
title: End a read-only link sooner
line: The Share dialog lists a session's read-only links with Revoke, and a link's page waits for a click before it connects.
---
Revoking a link ends whoever is watching through it at once, and it won't open again. So does closing its session. `arugula links` lists them and `arugula links revoke ID` ends one.

A link's page now says what it is and connects only when you click. A machine older than this release can't list or end its links from the dialog until it's updated; they still end on time.
