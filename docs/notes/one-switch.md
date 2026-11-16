---
section: Changed
title: One switch everywhere
line: A switch looks the same wherever it is: on the platform's Settings, on a machine's own, and in a machine's dialogs (Guest SSH access, Usage and crash reports), which drew a plain box before.
headline: no
---
The switch's look moved out of the Settings pages into its own file, web/src/shared/switch.css, so every `.sw-switch` gets it. The dialogs' on/off choices that take effect at once now use it.
