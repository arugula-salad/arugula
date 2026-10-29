# arugula.stream (presence/relay with presence/demo in priv/static). Built
# from presence/ (fly.toml there): the demo first, with its models and
# Wasm fetched, then the relay's release on the official Elixir image, run
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
RUN cd core && pnpm install --frozen-lockfile \
  && cd ../client && pnpm install --frozen-lockfile \
  && cd ../demo && pnpm install --frozen-lockfile \
  && sh ../core/scripts/assets.sh public \
  && mkdir -p ../relay/priv/static \
  && pnpm exec vite build

FROM hexpm/elixir:${ELIXIR}-erlang-${OTP}-debian-${DEBIAN} AS build
ENV MIX_ENV=prod
WORKDIR /app
RUN mix local.hex --force && mix local.rebar --force
COPY relay/mix.exs relay/mix.lock ./
RUN mix deps.get --only prod && mix deps.compile
COPY relay/config config
COPY relay/lib lib
COPY relay/priv priv
COPY --from=demo /src/relay/priv/static priv/static
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
