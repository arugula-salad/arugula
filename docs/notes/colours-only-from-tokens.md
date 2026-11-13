---
section: Changed
title: Every colour comes from the palette
line: Every colour in the app comes from the palette, so Latte and the finishes no longer show stray dark-theme greys or shadows.
headline: no
---
A check now fails the build on a raw colour in a stylesheet (`web/scripts/check-colour.mjs`): shadows, tints and borders are the palette's tokens, so they follow the scheme you picked.
