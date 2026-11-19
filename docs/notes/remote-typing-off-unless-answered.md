---
section: Changed
title: Typing through Arugula cloud is off unless you say so
line: "A machine that joins Arugula cloud without answering whether people may type into it through Arugula cloud starts with it off; you can still watch it from your other devices."
---
Unanswered means no terminal, a script, `arugulad join --token` without `--remote-input`, or the input ending. In all of those, a machine joining for the first time starts with remote typing off. An answer, `--remote-input on|off` or `ARUGULA_REMOTE_INPUT=on|off` still decides, and Getting started's question starts on No. The join says which way it went and how to allow typing: `arugulad sharing remote-input on` on the machine, or `--remote-input on` when joining. With it off, nobody types into the machine through Arugula cloud, you included, though you can still watch it from your other devices. Machines that joined before, and any machine whose owner already set it, keep what they have.
