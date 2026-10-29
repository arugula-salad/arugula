# arugula.video's relay (presence/relay): a release built on the official
# Elixir image, run on a slim Debian. The demo site, once built (#315
# step 3), goes into priv/static before `mix release`.
ARG ELIXIR=1.19.2
ARG OTP=28.3
ARG DEBIAN=bookworm-20251020-slim

FROM hexpm/elixir:${ELIXIR}-erlang-${OTP}-debian-${DEBIAN} AS build
ENV MIX_ENV=prod
WORKDIR /app
RUN mix local.hex --force && mix local.rebar --force
COPY mix.exs mix.lock ./
RUN mix deps.get --only prod && mix deps.compile
COPY config config
COPY lib lib
COPY priv priv
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
