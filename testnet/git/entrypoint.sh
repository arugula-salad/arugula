#!/bin/sh
# The git server's host key and the stack's client key from /keys, then sshd;
# with up.sh's certificate and password there, HTTPS too (M183).
set -eu
install -m 600 /keys/git_host_ed25519 /etc/ssh/ssh_host_ed25519_key
install -m 644 /keys/git_host_ed25519.pub /etc/ssh/ssh_host_ed25519_key.pub
install -d -m 700 -o git -g git /home/git/.ssh
install -m 600 -o git -g git /keys/id_ed25519.pub /home/git/.ssh/authorized_keys
if [ -f /keys/git_https.crt ] && [ -f /keys/git_https.key ] && [ -f /keys/git_htpasswd ]; then
  install -m 644 /keys/git_https.crt /etc/nginx/git-https.crt
  install -m 600 /keys/git_https.key /etc/nginx/git-https.key
  install -m 644 /keys/git_htpasswd /etc/nginx/git-htpasswd
  fcgiwrap -s unix:/run/fcgiwrap.socket &
  for _ in 1 2 3 4 5 6 7 8 9 10; do [ -S /run/fcgiwrap.socket ] && break; sleep 0.2; done
  chmod 666 /run/fcgiwrap.socket
  nginx
fi
exec /usr/sbin/sshd -D -e
