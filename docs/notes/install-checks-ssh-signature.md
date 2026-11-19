---
section: Changed
title: Installs check the release signature everywhere
line: install.sh on a stock Mac and install.ps1 on Windows now check a release's signed SHA256SUMS, with ssh-keygen.
headline: no
---
A daemon release's `SHA256SUMS` is now also signed with an SSH key
(`SHA256SUMS.sshsig`). `install.sh` checks it with `ssh-keygen -Y verify` where
openssl can't check Ed25519, as on a stock Mac, and `install.ps1` checks it with
the ssh-keygen that Windows 10 and later have. Where neither tool can check the
signature, both stop unless `ARUGULA_ALLOW_UNSIGNED=1`.
