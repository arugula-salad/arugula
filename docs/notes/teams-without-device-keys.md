---
section: Changed
title: Teams work without a device key
line: An owner signed in on any browser can make a team, invite people, change their roles, remove them and lock the team, with no other device to approve it.
needs: control
---
A team's members, roles, invite links and lock are Arugula cloud's now (aru-141): a change no longer needs a signature from one of the owner's devices, so someone who signed in on a new browser, or signed up without a device key, runs their team like anyone else. Making someone an owner, or removing one, still takes the owner's passkey. Team machines take these changes as Arugula cloud says, and a machine token's role follows them; machines from before this release stop at the first change made this way, so update them.
