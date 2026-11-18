---
section: Added
title: Agents run on Windows
line: On a Windows PC, Claude Code and Codex agents start, answer, ask for permission on your phone, and stop with everything they started.
---
Agent blocks on Windows used to fail at once with "agents don't run on Windows yet". The daemon now starts the agent's adapter itself, with no console window, in a job object: closing the block stops the adapter and every program it started, `claude` included. mise's Node is found in `%LOCALAPPDATA%\mise` there.

One difference from Linux and macOS: an agent doesn't outlive a daemon restart on Windows. The block starts it again and reopens its conversation, as after a reboot (or waits for *Resume*, by its restart setting).
