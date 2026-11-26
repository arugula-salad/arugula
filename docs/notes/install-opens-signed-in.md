---
section: Fixed
title: The installer opens your page signed in
line: Installing with the install script on a machine with a browser opens your machine's page on Your work, already signed in, instead of a page asking for its sign-in link.
headline: no
---
The script opens the page with `arugula web --if-browser`, which opens the sign-in link in this machine's browser and never prints it. Over ssh, with no browser, or with the Mac app (which shows the page itself) it opens nothing and says to run `arugula web`, as before. `ARUGULA_NO_OPEN=1` keeps it from opening.
