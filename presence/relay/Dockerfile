# arugula.stream (presence/relay with presence/demo, meet.arugula.stream
# (presence/meet), the script-tag SDK and the hosted SDK installs in
# priv/static). Built from presence/ (fly.toml there): the demo, with its
# models and Wasm fetched, meet into priv/static/meet, the SDK, the React
# package and the server SDKs' npm/pip/Go files (sdk/host.sh), the gem
# index on Ruby, then the relay's release on the official Elixir image, run
# on a slim Debian.
ARG ELIXIR=1.19.2
ARG OTP=28.3.3
ARG DEBIAN=bookworm-20260610-slim

FROM node:24-bookworm AS demo
RUN npm install -g pnpm@12.5.1
WORKDIR /src
COPY core core
COPY client client
COPY demo demo
COPY meet meet
COPY elements elements
COPY react react
COPY sdk sdk
# The demo first (it empties priv/static), then meet into
# priv/static/meet, then the script-tag SDK into priv/static/sdk/v0, then
# the installs (npm, py, go) beside it.
RUN cd core && pnpm install --frozen-lockfile \
  && cd ../client && pnpm install --frozen-lockfile \
  && cd ../demo && pnpm install --frozen-lockfile \
  && sh ../core/scripts/assets.sh public \
  && mkdir -p ../relay/priv/static \
  && pnpm exec vite build \
  && cd ../meet && pnpm install --frozen-lockfile \
  && pnpm exec vite build \
  && cd ../elements && pnpm install --frozen-lockfile \
  && pnpm exec vite build \
  && cd ../react && pnpm install --frozen-lockfile \
  && pnpm exec vite build --config vite.pack.config.ts \
  && sh ../sdk/host.sh ../relay/priv/static

FROM ruby:3.4-slim AS gems
WORKDIR /src
COPY sdk/ruby ruby
RUN gem install --silent rubygems-generate_index \
  && mkdir -p /out/gems/gems \
  && cd ruby && gem build --silent arugula_presence.gemspec -o /out/gems/gems/arugula_presence-$(ruby -e 'require "./lib/arugula_presence"; print ArugulaPresence::VERSION').gem \
  && gem generate_index --directory /out/gems

FROM hexpm/elixir:${ELIXIR}-erlang-${OTP}-debian-${DEBIAN} AS build
ENV MIX_ENV=prod
WORKDIR /app
RUN mix local.hex --force && mix local.rebar --force
COPY relay/mix.exs relay/mix.lock ./
RUN mix deps.get --only prod && mix deps.compile
COPY relay/config config
COPY relay/lib lib
COPY relay/priv priv
# The release's environment: node names and clustering on Fly (P2).
COPY relay/rel rel
COPY --from=demo /src/relay/priv/static priv/static
COPY --from=gems /out/gems priv/static/gems
RUN mix compile && mix release

FROM debian:${DEBIAN}
RUN apt-get update && apt-get install -y --no-install-recommends libstdc++6 openssl libncurses6 ca-certificates \
  && rm -rf /var/lib/apt/lists/*
ENV LANG=C.UTF-8 PORT=4000
WORKDIR /app
RUN useradd --system --home /app relay && chown relay /app
COPY --from=build --chown=relay /app/_build/prod/rel/presence_relay ./
USER relay
EXPOSE 4000
CMD ["/app/bin/presence_relay", "start"]
