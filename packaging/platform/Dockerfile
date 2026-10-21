# arugula-platform (M140): control's platform (chat, Cloud, billing,
# projects and grants) as its own process, behind the hub's gateway. The
# static binary (`just static`) on distroless, which brings CA certificates
# (Managoat, Stripe, PostHog) and nothing else; root, as Fly volumes are
# root-owned. Its own image, so control's (packaging/control) is unchanged.
# A release tag (scripts/ci-platform-deploy) or `just platform-deploy`
# builds the context and deploys to Fly.

# Litestream (M141), for platform.db's off-site backup, the same build as
# control's (packaging/control/Dockerfile): the platform runs it only when
# LITESTREAM_BUCKET is set (docs/control-ops.md, "Backups").
FROM alpine:3.22 AS litestream
ARG LITESTREAM=0.5.17
ARG LITESTREAM_SHA256=cfb371176d164437ae869f8351cfde49bd1804ae71c61923f75c9cba9c9c006d
RUN wget -qO /ls.tgz "https://github.com/benbjohnson/litestream/releases/download/v${LITESTREAM}/litestream-${LITESTREAM}-linux-x86_64.tar.gz" \
 && echo "${LITESTREAM_SHA256}  /ls.tgz" | sha256sum -c - \
 && tar -xzf /ls.tgz -C / litestream

FROM gcr.io/distroless/static-debian12
COPY arugula-platform /arugula-platform
COPY --from=litestream /litestream /litestream
EXPOSE 7700
ENTRYPOINT ["/arugula-platform"]
