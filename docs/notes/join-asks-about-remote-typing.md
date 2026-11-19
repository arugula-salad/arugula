---
section: Added
title: Joining asks whether people may type remotely
line: "Adding a machine to Arugula cloud asks whether people who reach it through Arugula cloud may type into it, and the machine menu shows which."
---
`arugulad join` asks on a terminal (or takes `--remote-input on|off`), and Getting started asks on the machine's own page. Answering no is the same as `arugulad sharing remote-input off`: nobody types there through Arugula cloud, you included, though you can still watch from your other devices. It doesn't cover the machine's own terminals and page, the tailnet, or a device of yours the machine trusts by its own key (`arugula login --device`, older apps). The answer is set on the machine only; nothing through Arugula cloud can change it. The machine menu says *Remote typing: on* or *off*.
