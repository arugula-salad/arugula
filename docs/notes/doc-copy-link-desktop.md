---
section: Fixed
title: Copy link works on a Doc in a pane
line: A Doc's Copy link in the panes copies its link in the desktop app and Safari, instead of saying it couldn't.
headline: no
---
Copy link asks the machine for the Doc's link, and WebKit refused a clipboard write that came after that wait. The copy now starts with the click and fills in when the link arrives. Share's confirm and the share dialog's copy work the same way.
