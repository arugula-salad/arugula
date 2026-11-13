---
section: Fixed
title: arugula fs cat reads the whole file
line: On a machine reached through control, arugula fs cat stopped after the first 1 MiB and said nothing; it now reads to the end, or says how far it got.
headline: no
---
`arugula fs cat` on a file larger than 1 MiB, on a machine reached through
control, printed only the first megabyte and exited 0: the channel didn't
carry the file's size, so the CLI took it for 0. The channel now carries the
size and offset, and `cat` without a size reads on until a short page. If a
page fails partway, it exits 1 and says how many bytes it wrote and the
`--offset` to resume from.
