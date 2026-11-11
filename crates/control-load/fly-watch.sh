#!/usr/bin/env bash
# What Fly sees of the staging apps during a load test (#530), as one JSON
# line every 15 s: each machine's memory in use, CPU (busy share of one
# vCPU), connections through Fly's proxy (against the fly.toml limits), and
# the hub's 95th-percentile response time at Fly's edge. Fly's Prometheus,
# with the signed-in flyctl's token.
#
#   crates/control-load/fly-watch.sh > fly.jsonl
#
# APPS (default: both staging apps), ORG (default: personal), EVERY (s).
set -euo pipefail
org=${ORG:-personal}
apps=${APPS:-arugula-control-loadtest|arugula-platform-loadtest}
every=${EVERY:-15}
tok=$(fly auth token 2>/dev/null | tail -1)
q() {
  curl -sG --max-time 10 "https://api.fly.io/prometheus/$org/api/v1/query" -H "Authorization: FlyV1 $tok" --data-urlencode "query=$1" |
    jq -c '[.data.result[]? | {app: .metric.app, instance: .metric.instance, v: (.value[1] | if test("^-?[0-9.e+-]+$") then (tonumber | . * 1000 | round / 1000) else null end)}]' 2>/dev/null ||
    echo null
}
while :; do
  mem=$(q "sum by (app, instance) (fly_instance_memory_mem_total{app=~\"$apps\"} - fly_instance_memory_mem_available{app=~\"$apps\"}) / 1e6")
  cpu=$(q "sum by (app, instance) (rate(fly_instance_cpu{app=~\"$apps\", mode!~\"idle|steal|iowait\"}[1m])) / 100")
  conns=$(q "sum by (app, instance) (fly_app_concurrency{app=~\"$apps\"})")
  p95=$(q "histogram_quantile(0.95, sum by (app, le) (rate(fly_edge_http_response_time_seconds_bucket{app=~\"$apps\"}[1m])))")
  printf '{"kind":"fly","at":%s,"mem_mb":%s,"cpu":%s,"conns":%s,"edge_p95_s":%s}\n' "$(( $(date +%s%N) / 1000000 ))" "$mem" "$cpu" "$conns" "$p95"
  sleep "$every"
done
