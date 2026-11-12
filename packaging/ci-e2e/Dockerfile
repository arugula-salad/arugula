# The image check.yml's e2e shards and pull-request.yml's pr-e2e run in
# (`container:`): Playwright's own, with its browsers and every system
# package they need already in, plus what .github/actions/setup would
# otherwise fetch for them. So a shard downloads nothing from the runners'
# Ubuntu mirror, which once held shards for hours while it fetched WebKit's
# packages (https://app.arugula.io/issues/iss_db99400ba2c2d962bd012371678e2cb7).
#
# scripts/ci-e2e-image builds it, tagged with the Playwright version and a
# hash of this file, and pushes it to GHCR when the tag isn't there yet;
# PLAYWRIGHT is web's @playwright/test from web/pnpm-lock.yaml, so the
# browsers here are the revisions the specs ask for (docs/testing.md,
# "CI's images").
ARG PLAYWRIGHT
FROM mcr.microsoft.com/playwright:v${PLAYWRIGHT}-noble
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ARG PLAYWRIGHT

# Ubuntu 24.04, as ubuntu-24.04's runner, so the run-debug binaries built
# there run here (the same glibc). Its Node is 24 already.
#
# What the setup action apt-installs: vi and nvim for the layout spec,
# libnss-myhostname so Node resolves *.localhost (browser-ports' sites),
# zstd for actions/cache. python3 (scripts/free-port, the fake agents) and
# procps (ps, pgrep) the specs run, which the runner's image has and
# Ubuntu's base image doesn't. sudo for the steps that use it when
# something is missing. Playwright's image points apt at
# azure.archive.ubuntu.com, the mirror that stalls; this build is the only
# time it's used, and it's bounded.
RUN apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
      vim-tiny libnss-myhostname zstd python3 procps sudo xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && grep -q myhostname /etc/nsswitch.conf

# The job runs as pwuser (`options: --user 1001`), the runner's own uid, so
# the checkout and _temp it mounts are its own, and the specs run as an
# ordinary user, as they do on the runner (root ignores file modes). sudo
# as on the runner.
RUN [ "$(id -u pwuser)" = 1001 ] \
    && echo 'pwuser ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/pwuser \
    && chmod 440 /etc/sudoers.d/pwuser

# nvim from its release, the setup action's pin and checksums: its nvim
# step finds this one and fetches nothing.
RUN set -eu; v=0.11.6; \
    case "$(dpkg --print-architecture)" in \
      amd64) a=x86_64 sum=2fc90b962327f73a78afbfb8203fd19db8db9cdf4ee5e2bef84704339add89cc ;; \
      arm64) a=arm64 sum=8ddc0c101846145e830b17bbca50782ca9307eee4fab539d9e2ddaf8793c06f1 ;; \
      *) echo "no nvim for this architecture" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/nvim.tar.gz "https://github.com/neovim/neovim/releases/download/v$v/nvim-linux-$a.tar.gz"; \
    echo "$sum  /tmp/nvim.tar.gz" | sha256sum -c --quiet; \
    mkdir -p "/opt/nvim-$v" && tar -xzf /tmp/nvim.tar.gz -C "/opt/nvim-$v" --strip-components 1; \
    ln -s "/opt/nvim-$v/bin/nvim" /usr/local/bin/nvim; \
    rm /tmp/nvim.tar.gz; \
    nvim --version | head -1

# just, from its release (the setup action installs the newest with
# taiki-e/install-action when there's none).
RUN set -eu; v=1.59.0; \
    case "$(dpkg --print-architecture)" in \
      amd64) a=x86_64 sum=409044617b6f06517c1035e55ca38c8c856ff8b650e41bc2aa1ef82d5903a23e ;; \
      arm64) a=aarch64 sum=23871b1702bfd550fb5f901778fd4290ecf2ee034a45a889fbe96d89b188086d ;; \
      *) echo "no just for this architecture" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/just.tar.gz "https://github.com/casey/just/releases/download/$v/just-$v-$a-unknown-linux-musl.tar.gz"; \
    echo "$sum  /tmp/just.tar.gz" | sha256sum -c --quiet; \
    tar -xzf /tmp/just.tar.gz -C /usr/local/bin just; \
    rm /tmp/just.tar.gz; \
    just --version

# pnpm 12 (the setup action's pnpm/action-setup) and Claude Code, which the
# specs that set it up (journey-j5) find on PATH: the setup action's
# version, whose step skips installing it when this one's is it.
RUN npm install -g --silent pnpm@12.18.0 @anthropic-ai/claude-code@2.1.296 \
    && rm -rf /root/.npm \
    && pnpm --version && claude --version

# `just browsers` and the e2e-shard action skip installing the browsers'
# system packages when this is here (they're in Playwright's image).
RUN mkdir -p /etc/arugula-ci && echo "mcr.microsoft.com/playwright:v${PLAYWRIGHT}-noble" > /etc/arugula-ci/browser-deps
