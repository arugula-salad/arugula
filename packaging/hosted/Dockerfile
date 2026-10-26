# A hosted workspace's box (M177, #1238): the static arugulad and arugula
# (`just static`), the ACP adapters Claude Code and Codex run through, Codex
# itself, Node, git, gh and flyctl, all pinned here. A release builds it as E2B's
# template `arugula-workspace`, tagged with its version, snapshotted with
# `arugulad --hosted` already running (scripts/ci-hosted-template). The same
# file builds the image loam runs, FROM loam's e2b image instead
# (`ci-hosted-template image DIR BASE` swaps the FROM line).
#
# One stage, no build flags: E2B's Template SDK reads this file itself
# (fromDockerfile), and it takes neither multi-stage builds, COPY flags nor
# an ARG in FROM.
#
# An ARG's default is its pin; the release changes nothing here, only the
# binaries beside it are the tag's. CLAUDE_ACP and CODEX_ACP are the
# daemon's own pins (crates/daemon/src/agent/defs.rs), checked by a test.
#
# Nothing in here is a box's own: no key, no ticket, no identity. arugulad
# makes its device key only once it sees a sandbox id that isn't the
# snapshot's (M180).
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

# Ubuntu's archive as it was on this day (snapshot.ubuntu.com): git and the
# rest come from it. Its certificates come from today's archive, since the
# snapshot is https.
ARG APT_SNAPSHOT=20261020T000000Z
ARG NODE_VERSION=22.23.3
ARG NODE_SHA256=df450af89261115ef9f9e3830c3eeb2cc9213b63c720b1af623cb5dcbe2e02de
ARG GH_VERSION=2.103.0
ARG GH_SHA256=fdd768eb70f6409f80fcfa9f3a1c0adf0fc98d827ab3ce0bed2ac22762c5d786
# flyctl, which the chant→Fly deploy driver runs (M220): the version
# release.yml's Fly deploys use.
ARG FLYCTL_VERSION=0.4.104
ARG FLYCTL_SHA256=4023d2a4f5e76198197f80bde74271184b200b9034bb16d799acba373714bfbc
ARG CLAUDE_ACP=@agentclientprotocol/claude-agent-acp@0.85.0
ARG CODEX_ACP=@agentclientprotocol/codex-acp@2.1.0
ARG CODEX=@openai/codex@0.163.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends ca-certificates \
 && apt-get update -qq --snapshot "$APT_SNAPSHOT" \
 && apt-get install -y -qq --no-install-recommends \
      bash curl git iptables less nftables openssh-client procps sudo tmux xz-utils \
 && rm -rf /var/lib/apt/lists/*

RUN set -eu; cd /tmp; f="node-v$NODE_VERSION-linux-x64.tar.xz"; \
    curl -fsSLO "https://nodejs.org/dist/v$NODE_VERSION/$f"; \
    echo "$NODE_SHA256  $f" | sha256sum -c -; \
    tar -xJf "$f" -C /usr/local --strip-components=1 --no-same-owner \
      --exclude CHANGELOG.md --exclude LICENSE --exclude README.md; \
    rm "$f"; node --version

RUN set -eu; cd /tmp; f="gh_${GH_VERSION}_linux_amd64.tar.gz"; \
    curl -fsSLO "https://github.com/cli/cli/releases/download/v$GH_VERSION/$f"; \
    echo "$GH_SHA256  $f" | sha256sum -c -; \
    tar -xzf "$f" --strip-components=2 -C /usr/local/bin "gh_${GH_VERSION}_linux_amd64/bin/gh"; \
    rm "$f"; gh --version

RUN set -eu; cd /tmp; f="flyctl_${FLYCTL_VERSION}_Linux_x86_64.tar.gz"; \
    curl -fsSLO "https://github.com/superfly/flyctl/releases/download/v$FLYCTL_VERSION/$f"; \
    echo "$FLYCTL_SHA256  $f" | sha256sum -c -; \
    tar -xzf "$f" -C /usr/local/bin flyctl; \
    rm "$f"; flyctl version

# Codex's CLI, which codex-acp drives (the daemon installs codex-acp without
# its bundled copy).
RUN npm install -g --no-fund --no-audit "$CODEX" && npm cache clean --force && codex --version

# The workspace's user, `user` at uid 1000 with sudo, as on E2B's base (whose
# build makes it before this runs; Ubuntu's image has `ubuntu` there).
# `arugulad _reseed` (M180) is named on its own, so a new box from the
# snapshot can still reseed the kernel's randomness before it makes any key
# if the blanket rule ever goes.
RUN id -u user >/dev/null 2>&1 || { userdel -r ubuntu 2>/dev/null; useradd -m -u 1000 -s /bin/bash user; } \
 && echo 'user ALL=(ALL:ALL) NOPASSWD: ALL' > /etc/sudoers.d/user && chmod 0440 /etc/sudoers.d/user \
 && echo 'user ALL=(root) NOPASSWD: /usr/local/bin/arugulad _reseed *' > /etc/sudoers.d/arugulad-reseed \
 && chmod 0440 /etc/sudoers.d/arugulad-reseed

# Runs the platform starts (M212) run as `runner`, with `sudo -n -u runner`
# from arugulad, an empty environment and a pty of their own:
# - no sudo of its own;
# - in `user`'s group, so it reads /home/user (0750) and the repos there
#   (cloned with the default umask: group-readable, never group-writable),
#   and works only in its own copy under /home/runner/work;
# - never arugulad's state directory (0700), where its keys, its enrollment
#   and the grant revokes it heard are;
# - kept off the daemon's and the provider's ports on loopback by arugulad's
#   nftables table (`meta skuid`, so `nft` is installed above; E2B's base
#   has it, loam's doesn't), else iptables rules (owner match), made before
#   its first run.
RUN { id -u runner >/dev/null 2>&1 || useradd -m -s /bin/bash runner; } && usermod -aG user runner && chmod 0750 /home/user \
 && echo 'Defaults>runner env_reset, use_pty' > /etc/sudoers.d/runner && chmod 0440 /etc/sudoers.d/runner

COPY arugulad /usr/local/bin/arugulad
COPY arugula /usr/local/bin/arugula
COPY ready /usr/local/bin/arugula-hosted-ready
# loam's service definition: loam starts it at every boot, as E2B's snapshot
# has it running (loam has no snapshot of a running template). Inert on E2B.
COPY arugulad.json /var/lib/loam/services/arugulad.json
RUN chmod 0755 /usr/local/bin/arugulad /usr/local/bin/arugula /usr/local/bin/arugula-hosted-ready \
 && chmod 0644 /var/lib/loam/services/arugulad.json \
 && arugulad --version && arugula --version

# ---- M258 (Arugula #124): the daemon is supervised ----------------------
# Nothing else in a box restarts `arugulad --hosted` when it exits, and a
# pause keeps a dead one dead. The start command (template.mjs, loam's
# arugulad.json) is `arugula-hosted-start`: with systemd as PID 1 it starts
# arugulad.service (Restart=always, OOMScoreAdjust=-1000, enabled here so it
# starts at every boot); without, it's the supervisor itself. Checked by
# scripts/tests/hosted-supervisor.sh. Other blocks go above or below this
# one, not in it.
COPY start /usr/local/bin/arugula-hosted-start
COPY arugulad.service /etc/systemd/system/arugulad.service
RUN chmod 0755 /usr/local/bin/arugula-hosted-start && chmod 0644 /etc/systemd/system/arugulad.service \
 && mkdir -p /etc/systemd/system/multi-user.target.wants \
 && ln -sf /etc/systemd/system/arugulad.service /etc/systemd/system/multi-user.target.wants/arugulad.service
# ---- end M258 ------------------------------------------------------------

# --- M255 (thread = session): session users. ---------------------------
# A hosted session's agent runs as one of ses1..ses3, one per session the
# box runs at once (the daemon's cap), each kept by its session for life:
# `sudo -n -u sesN -- env -i …` from arugulad, for the agent and for all of
# the daemon's git in the session's checkout (a clone in ~sesN/work).
# - no sudo of its own, and env_reset;
# - not in `user`'s group, unlike `runner`: /home/user (0750), setup's
#   files there and the daemon's state are closed to it;
# - its home 0700, closed to the other sessions;
# - kept off loopback (the daemon's and the provider's ports) with runner,
#   by arugulad's nftables table, before the first session runs.
# The adapters live in /opt/arugula/agents (below), which they can read.
RUN for n in 1 2 3; do { id -u "ses$n" >/dev/null 2>&1 || useradd -m -s /bin/bash "ses$n"; } \
      && chmod 0700 "/home/ses$n"; done \
 && printf 'Defaults>ses1 env_reset\nDefaults>ses2 env_reset\nDefaults>ses3 env_reset\n' > /etc/sudoers.d/sessions \
 && chmod 0440 /etc/sudoers.d/sessions \
 && install -d -o user -g user -m 0755 /opt/arugula /opt/arugula/agents
# --- end M255 -----------------------------------------------------------

# The adapters: in /opt/arugula/agents (M255: owned by `user`, who updates
# them, and readable by the session users, who can't reach /home/user),
# linked from where the daemon looks first (agent/adapters.rs). A session's
# agent is started by the link's target.
USER user
WORKDIR /home/user
RUN npm install --no-fund --no-audit --prefix /opt/arugula/agents/claude "$CLAUDE_ACP" \
 && npm install --no-fund --no-audit --omit=optional --prefix /opt/arugula/agents/codex "$CODEX_ACP" \
 && npm cache clean --force \
 && mkdir -p /home/user/.local/share/arugula \
 && ln -s /opt/arugula/agents /home/user/.local/share/arugula/agents
