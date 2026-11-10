---
section: Fixed
title: A first visit to a machine's page is quieter
line: The usage-reports card waits until Getting started is closed, Edit in nvim says whether nvim is here, and the page no longer gets a 401 from the daemon as it loads.
headline: no
---
On a machine's own page, the card asking about usage reports no longer sits
behind Getting started's greeting: it shows once no dialog is up. Getting
started's *Edit in nvim* card names itself once, and says "Nothing to install"
only when the daemon finds `nvim` on its PATH; otherwise it says nvim is
needed. As the page loads it asks whether control serves it, and a daemon now
answers 404 rather than 401 (which also logged a rejected request each time).
