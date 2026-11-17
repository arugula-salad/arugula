---
section: Changed
title: Settings: one page for you, a machine, a project and a team
line: Settings is one page with four tabs (You, This machine, Project, Team and billing), each a list of sections saying what's set, with the one you pick beside it.
try: {"route": "/settings"}
needs: control
---
Each section in the list says what's set in a few words, such as "Weeknights 10 PM – 8 AM" or "acme/shop", so you can see what's on without opening it. Pick one and its controls open beside the list, with a line under the heading saying what it's for; the More toggles are gone. A picker under the tabs chooses which machine, project or team. On a phone you see the list first, and a section opens full width with a way back.

Every section has its own address, such as `/settings/you/quiet-hours` or `/settings/project/<project>/repos`, so a link opens it. Old addresses still work: a project's settings (`/p/<project>/settings`), Personal's (`/settings/personal`) and a team's (`/t/<team>/settings`) open the matching tab. The rail's Settings opens the project you're in, and "Your account…" under your picture opens You. This machine links to the machine's own page for now.
