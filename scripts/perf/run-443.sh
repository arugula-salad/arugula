#!/usr/bin/env bash
# Measurement only (#443): the tab-switch spec direct and through scripts/perf/link-proxy.mjs.
# Usage: run-443.sh OUTFILE LABEL   (from web/; env RUNS=3, CASES="direct p0 4m 1m")
set -u
out=$1; label=$2; runs=${RUNS:-3}; cases=${CASES:-direct p0 4m 1m}
DAEMON=7693; PROXY=7694
for c in $cases; do
  case $c in direct) args="";; p0) args="0 0";; 4m) args="60 4";; 1m) args="60 1";; esac
  for r in $(seq 1 "$runs"); do
    ppid=""
    if [ "$c" != direct ]; then
      node ../scripts/perf/link-proxy.mjs $PROXY $DAEMON $args & ppid=$!
      sleep 1; export PERF_PROXY_PORT=$PROXY
    else unset PERF_PROXY_PORT; fi
    PERF_SWITCHES=${SW:-5} PERF_LABEL="$label/$c/run$r" E2E_PORT=$DAEMON ../scripts/suite-lock perl -e '$p=fork||do{setpgrp;exec @ARGV};$SIG{ALRM}=sub{kill TERM=>-$p;sleep 5;kill KILL=>-$p;exit 142};alarm 900;waitpid $p,0;exit $?>>8' pnpm exec playwright test e2e/perf-pane-drag-tab-switch.spec.ts -g "tab switch" 2>&1 | grep -E "PERF|DEBUG|passed|failed|Error" | cut -c1-1500 >>"$out"
    [ -n "$ppid" ] && kill "$ppid" 2>/dev/null && wait "$ppid" 2>/dev/null
  done
done
echo DONE >>"$out"
