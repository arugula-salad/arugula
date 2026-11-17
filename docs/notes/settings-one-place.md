---
section: Changed
title: Settings is in one place
line: Settings on your machines page now opens the app's Settings, and old links to its sections land there. Only Phones and browsers stays on your machines page.
try: {"route": "/settings"}
needs: control
---
Your machines page's Settings now opens the app's Settings. Old `#settings` links land on the matching section: Appearance, Preferences and Account under You, and a machine's own settings under This machine. Remote sessions, the list of who reached a machine through Arugula cloud, is now a section of This machine.

Phones and browsers stays on your machines page, because a device turns its notifications on with its own keys. The ↗ rows in Settings still open it there.

On your machines page, a machine's menu lists its settings again (Access, Permission rules and the rest). In the app, the menu opens This machine in Settings. A machine's own page, served by its daemon with no Arugula cloud, keeps its whole Settings page.
