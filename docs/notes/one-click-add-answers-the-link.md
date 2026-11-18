---
section: Changed
title: Adding a machine takes its whole link
line: "A one-click Add answers the secret in the link `arugulad join` prints, and `arugulad join --account` refuses an Add into another account."
headline: no
---
When a machine's join link carries a secret (the part after the dot), the
page's Add sends an answer to it, and the machine checks that answer before it
saves the account it was added to. Open the whole link the machine printed,
or type the whole code; a page that has only the code asks for the rest
before it offers Add.

`arugulad join --account FINGERPRINT` now holds for a one-click Add too: the
account must be that one, by its id or its root device's, or the machine stays
unjoined and says which account added it. `arugulad join --token` is
unchanged.
