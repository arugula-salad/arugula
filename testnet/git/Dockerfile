# A git server: bare repositories over ssh, as GitHub or Forgejo serve them
# for `git push`. One user, git, whose shell is git-shell and who logs in
# with the stack's key only. M51's `git push` from a pane on a box reaches
# it with the agent forwarded through `arugula --ssh`; see testnet/README.md.
# The same repositories over HTTPS too (git-http-backend behind nginx, basic
# auth), as a forge serves clones with a token: a hosted workspace's setup
# clones from here (M183).
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server git nginx-light fcgiwrap \
 && rm -rf /var/lib/apt/lists/* /etc/ssh/ssh_host_* \
 && mkdir -p /run/sshd /srv/git \
 && useradd -m -s /usr/bin/git-shell git \
 && usermod -p '*' git \
 && git init -q --bare -b main /srv/git/repo.git \
 && chown -R git:git /srv/git \
 && git config --system --add safe.directory '*' \
 && rm -f /etc/nginx/sites-enabled/default
COPY https.conf /etc/nginx/conf.d/git-https.conf
COPY sshd_config /etc/ssh/sshd_config
COPY entrypoint.sh /entrypoint.sh
EXPOSE 22 443
ENTRYPOINT ["/entrypoint.sh"]
