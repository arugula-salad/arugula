# arugula-platform (M140): control's platform (chat, hosted workspaces,
# billing, projects and grants) as its own process, behind the hub's
# gateway. The static binary (`just static`) on distroless, which brings CA
# certificates (E2B, Stripe, PostHog) and nothing else. Its own image, so
# control's (packaging/control) is unchanged. A release tag
# (scripts/ci-platform-deploy) or `just platform-deploy` builds the context
# and deploys to Fly.
#
# No Litestream (Arugula #72): hosted control's platform is on Postgres with no
# volume (fly.toml), and Litestream only ever backed up a SQLite file. The
# nightly dump (packaging/platform-dump) backs Postgres up.
FROM gcr.io/distroless/static-debian12
COPY arugula-platform /arugula-platform
EXPOSE 7700
ENTRYPOINT ["/arugula-platform"]
