# arugula-platform (M140): control's platform (chat, Cloud, billing,
# projects and grants) as its own process, behind the hub's gateway. The
# static binary (`just static`) on distroless, which brings CA certificates
# (Managoat, Stripe, PostHog) and nothing else; root, as Fly volumes are
# root-owned. Its own image, so control's (packaging/control) is unchanged.
# A release tag (scripts/ci-platform-deploy) or `just platform-deploy`
# builds the context and deploys to Fly.
FROM gcr.io/distroless/static-debian12
COPY arugula-platform /arugula-platform
EXPOSE 7700
ENTRYPOINT ["/arugula-platform"]
