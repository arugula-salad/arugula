# Arugula app 0.32.3

- **Security updates** (#2040, #2042). The updater's `zip` library moves to 9.2.2, which clears a critical advisory (GHSA-m665-v7pm-7grj) and two others, and `rustls` moves to 0.23.46. The app only reads zip archives when updating on Windows, after checking the update's signature, so the risk was low, but there's no reason to keep them.

It carries the newest arugulad release.
