#!/bin/sh
# The People block's models and Wasm into web/public, which control serves (presence/core/scripts/assets.sh).
exec "$(dirname "$0")/../../presence/core/scripts/assets.sh" "$(dirname "$0")/../public"
