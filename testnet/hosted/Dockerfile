# A hosted workspace's box for the `runs` claim (M212): the users M177's
# template has (packaging/hosted/Dockerfile), and nothing else. `user` runs
# `arugulad --hosted` with sudo; runs run as `runner`, which has no sudo,
# reads /home/user through `user`'s group, and never arugulad's state. The
# claim starts the daemon with the stack's static binaries (/opt/arugula)
# and a ticket from control.
FROM debian:bookworm-slim
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends bash ca-certificates git iptables procps sudo \
 && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1000 -s /bin/bash user \
 && echo 'user ALL=(ALL:ALL) NOPASSWD: ALL' > /etc/sudoers.d/user && chmod 0440 /etc/sudoers.d/user \
 && useradd -m -u 1001 -s /bin/bash runner && usermod -aG user runner && chmod 0750 /home/user \
 && echo 'Defaults>runner env_reset, use_pty' > /etc/sudoers.d/runner && chmod 0440 /etc/sudoers.d/runner
CMD ["sleep", "infinity"]
