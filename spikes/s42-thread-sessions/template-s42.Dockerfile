# S42 (prototype, not for main): what the spike adds to the hosted
# template (packaging/hosted/Dockerfile), appended to it by rig.sh.
# - python3, for fake_acp.py (the daemon's fake ACP agent);
# - three session users, ses1..ses3, one per session a box runs at once:
#   no sudo, not in `user`'s group (so not /home/user, unlike `runner`),
#   homes 0700, and sudo resets their environment;
# - fake_acp.py and session-proto.sh in /opt/s42.
USER root
RUN apt-get update -qq --snapshot "$APT_SNAPSHOT" \
 && apt-get install -y -qq --no-install-recommends python3 \
 && rm -rf /var/lib/apt/lists/*
RUN for n in 1 2 3; do useradd -m -s /bin/bash "ses$n" && chmod 0700 "/home/ses$n"; done \
 && printf 'Defaults>ses1 env_reset\nDefaults>ses2 env_reset\nDefaults>ses3 env_reset\n' > /etc/sudoers.d/sessions \
 && chmod 0440 /etc/sudoers.d/sessions
COPY fake_acp.py /opt/s42/fake_acp.py
COPY session-proto.sh /opt/s42/session-proto.sh
RUN chmod 0755 /opt/s42 && chmod 0644 /opt/s42/fake_acp.py /opt/s42/session-proto.sh
USER user
WORKDIR /home/user
