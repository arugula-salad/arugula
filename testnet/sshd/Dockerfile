# A plain Debian box with OpenSSH, git and one user, illo, and no arugula.
# The bastion and box-bare both run it; see testnet/README.md.
# Docker Hub's image, from CI's mirror (docs/testing.md, "CI's images").
ARG ARUGULA_IMAGES=ghcr.io/arugula-salad/ci
FROM ${ARUGULA_IMAGES}/debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server openssh-client git procps \
 && rm -rf /var/lib/apt/lists/* /etc/ssh/ssh_host_* \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash illo \
 && usermod -p '*' illo
COPY sshd_config /etc/ssh/sshd_config
COPY entrypoint.sh /entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/entrypoint.sh"]
