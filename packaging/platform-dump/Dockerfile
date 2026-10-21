# The platform's nightly dump (M145, docs/control-ops.md "The nightly
# dump"): a scheduled machine in the arugula-platform app that runs
# /platform-dump once a day and exits. pg_dump 17 from Postgres's own image
# (the cluster is Fly Managed Postgres 17; pg_dump must be at least the
# server's major version), age to encrypt to the recipients' public keys,
# curl to put it in R2 (SigV4), jq for the record beside it. The
# arugula-platform binary is the same build the platform runs, for its
# read-only window tools: `migrations` and `checksums --snapshot`.
# scripts/ci-platform-deploy builds it from the release's binary and moves
# the dump machine onto it after each platform deploy.
FROM postgres:17.11-alpine3.24
RUN apk add --no-cache age curl jq
COPY arugula-platform /arugula-platform
COPY dump.sh /platform-dump
ENTRYPOINT ["/platform-dump"]
