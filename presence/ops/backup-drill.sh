#!/usr/bin/env bash
# The backup drill (#318's P12; README.md, "Backups"): dump a database,
# restore it into a scratch database, check the copy against the source,
# and drop the scratch database.
#
#   SOURCE_URL=postgres://user:pass@127.0.0.1:16380/presence_staging \
#   SCRATCH_ADMIN_URL=postgres://postgres:postgres@127.0.0.1:55432/postgres \
#     ops/backup-drill.sh
#
# SOURCE_URL         the database to back up (staging: through `fly mpg proxy`).
#                    Only read: the drill never writes to it.
# SCRATCH_ADMIN_URL  any database on a server where the drill may CREATE and
#                    DROP the scratch database (a local Docker Postgres).
# SCRATCH_DB         the scratch database's name (default presence_drill_<time>);
#                    it must start with presence_drill, and mustn't exist yet.
# DUMP_DIR           where the dump goes (default: a new temp dir); the dump is
#                    deleted at the end unless KEEP_DUMP=1.
# PG_DOCKER          an image (postgres:17) to run pg_dump, pg_restore and psql
#                    in, with the host's network, when the local ones are
#                    missing or older than the server (each run is a container
#                    named presence-drill-<pid>-<random>, plus PG_DOCKER_SUFFIX).
#
# The checks: the same tables; per table the same row count and the same
# checksum of every row (md5 over the rows' text, in order); the same
# constraints and indexes; every sequence at or past its column's highest
# value (so inserts after a restore don't collide); the last migration;
# and a few of the data's own rules (no orphans, usage days agreeing with
# usage members). The source's side is read in the very snapshot pg_dump
# dumps (an exported snapshot), so a live database's writes during the
# drill can't make a false difference.
#
# Exit 0 when every check passes; 1 when one fails; 2 on a usage error.
set -euo pipefail

: "${SOURCE_URL:?SOURCE_URL: the database to back up}"
: "${SCRATCH_ADMIN_URL:?SCRATCH_ADMIN_URL: a database on the server the scratch copy goes to}"
SCRATCH_DB="${SCRATCH_DB:-presence_drill_$(date -u +%Y%m%d%H%M%S)}"
case "$SCRATCH_DB" in
  presence_drill*) ;;
  *) echo "SCRATCH_DB must start with presence_drill (got $SCRATCH_DB)" >&2; exit 2 ;;
esac
[[ "$SCRATCH_DB" =~ ^[a-z0-9_]+$ ]] || { echo "SCRATCH_DB: lower-case letters, digits and _ only" >&2; exit 2; }

own_dir=0
if [[ -n "${DUMP_DIR:-}" ]]; then
  mkdir -p "$DUMP_DIR"
else
  DUMP_DIR="$(mktemp -d -t presence-drill.XXXXXX)"
  own_dir=1
fi
DUMP="$DUMP_DIR/$SCRATCH_DB.dump"

# The scratch database's URL: the admin URL with its database swapped.
SCRATCH_URL="$(printf '%s' "$SCRATCH_ADMIN_URL" | sed -E "s#^([^?]*/)[^/?]*(\?.*)?\$#\1$SCRATCH_DB\2#")"

pg() {
  if [[ -n "${PG_DOCKER:-}" ]]; then
    docker run --rm -i --network host --name "presence-drill-$BASHPID-$RANDOM${PG_DOCKER_SUFFIX:-}" \
      -v "$DUMP_DIR:$DUMP_DIR" "$PG_DOCKER" "$@"
  else
    "$@"
  fi
}
q() { pg psql "$1" -X -q -At -v ON_ERROR_STOP=1 -c "$2"; }

say() { printf '[drill %s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
fails=0
check() { # name, expected, got
  if [[ "$2" == "$3" ]]; then
    printf '  ok    %s\n' "$1"
  else
    printf '  FAIL  %s\n        source:  %s\n        restored: %s\n' "$1" "$2" "$3"
    fails=$((fails + 1))
  fi
}

created=0
cleanup() {
  if [[ "$created" == 1 ]]; then
    say "dropping $SCRATCH_DB"
    q "$SCRATCH_ADMIN_URL" "DROP DATABASE IF EXISTS $SCRATCH_DB WITH (FORCE)" >/dev/null || say "couldn't drop $SCRATCH_DB: drop it by hand"
  fi
  if [[ "${KEEP_DUMP:-0}" == 1 ]]; then
    say "dump kept: $DUMP"
  else
    rm -f "$DUMP"
    if [[ "$own_dir" == 1 ]]; then rmdir "$DUMP_DIR" 2>/dev/null || true; fi
  fi
}
trap cleanup EXIT

# What's compared, as queries that print one line per thing. The tables'
# per-table query is built from the table list.
TABLES_Q="SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY 1"
CONSTRAINTS_Q="SELECT conrelid::regclass::text || ' ' || contype::text || ' ' || conname FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace WHERE n.nspname = 'public' ORDER BY 1"
INDEXES_Q="SELECT indexname FROM pg_indexes WHERE schemaname = 'public' ORDER BY 1"
MIGRATION_Q="SELECT coalesce(max(version)::text, 'none') FROM schema_migrations"

per_table_q() { # tables... → one query: name, count, md5 of the rows in order
  local parts=()
  for t in "$@"; do
    parts+=("SELECT '$t' || ' ' || count(*) || ' ' || coalesce(md5(string_agg(r::text, E'\\n' ORDER BY r::text)), '-') FROM public.\"$t\" r")
  done
  local IFS=$'\n'
  printf '%s\nUNION ALL\n' "${parts[@]}" | sed '$d'
  printf ' ORDER BY 1'
}

# 1. The source, in one snapshot: its tables, rows and checksums, then
#    pg_dump of that same snapshot.
say "reading the source in one snapshot"
coproc SNAP { pg psql "$SOURCE_URL" -X -q -At -v ON_ERROR_STOP=1; }
send() {
  [[ -n "${SNAP[1]:-}" ]] || { say "the source's psql stopped (an error above)"; exit 1; }
  printf '%s;\n' "$1" >&"${SNAP[1]}"
}
recv_until() { # read lines until the marker line
  local line out=()
  while IFS= read -r -t 600 line <&"${SNAP[0]}"; do
    [[ "$line" == "$1" ]] && break
    out+=("$line")
  done
  printf '%s\n' "${out[@]}"
}
send "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY"
send "SELECT pg_export_snapshot()"
read -r -t 60 SNAPSHOT <&"${SNAP[0]}" || { say "the source didn't answer"; exit 1; }
[[ "$SNAPSHOT" =~ ^[0-9A-F-]+$ ]] || { say "couldn't export a snapshot: $SNAPSHOT"; exit 1; }

send "$TABLES_Q"; send "SELECT '--end--'"
mapfile -t TABLES < <(recv_until --end--)
[[ ${#TABLES[@]} -gt 0 ]] || { say "the source has no tables in public"; exit 1; }
send "$(per_table_q "${TABLES[@]}")"; send "SELECT '--end--'"
SRC_ROWS="$(recv_until --end--)"
send "$CONSTRAINTS_Q"; send "SELECT '--end--'"
SRC_CONSTRAINTS="$(recv_until --end--)"
send "$INDEXES_Q"; send "SELECT '--end--'"
SRC_INDEXES="$(recv_until --end--)"
send "$MIGRATION_Q"; send "SELECT '--end--'"
SRC_MIGRATION="$(recv_until --end--)"

say "pg_dump (snapshot $SNAPSHOT) → $DUMP"
t0=$(date +%s)
pg pg_dump --format=custom --no-owner --no-privileges --snapshot="$SNAPSHOT" --file="$DUMP" "$SOURCE_URL"
send "COMMIT"
exec {SNAP[1]}>&-
wait "$SNAP_PID" 2>/dev/null || true
say "dumped in $(( $(date +%s) - t0 )) s: $(du -h "$DUMP" | cut -f1), ${#TABLES[@]} tables"

# 2. Restore into the scratch database.
if [[ "$(q "$SCRATCH_ADMIN_URL" "SELECT count(*) FROM pg_database WHERE datname = '$SCRATCH_DB'")" != 0 ]]; then
  say "$SCRATCH_DB exists already: pick another SCRATCH_DB"; exit 2
fi
say "creating $SCRATCH_DB and restoring"
q "$SCRATCH_ADMIN_URL" "CREATE DATABASE $SCRATCH_DB" >/dev/null
created=1
t0=$(date +%s)
pg pg_restore --no-owner --no-privileges --exit-on-error --dbname="$SCRATCH_URL" "$DUMP"
say "restored in $(( $(date +%s) - t0 )) s"

# 3. Check the copy.
echo "checks ($SCRATCH_DB against the source's snapshot):"
check "tables" "$(printf '%s\n' "${TABLES[@]}")" "$(q "$SCRATCH_URL" "$TABLES_Q")"
DST_ROWS="$(q "$SCRATCH_URL" "$(per_table_q "${TABLES[@]}")")"
while read -r t n sum; do
  got="$(grep -E "^$t " <<<"$DST_ROWS" || true)"
  check "$t: $n rows, checksum" "$t $n $sum" "$got"
done <<<"$SRC_ROWS"
check "constraints ($(wc -l <<<"$SRC_CONSTRAINTS"))" "$SRC_CONSTRAINTS" "$(q "$SCRATCH_URL" "$CONSTRAINTS_Q")"
check "indexes ($(wc -l <<<"$SRC_INDEXES"))" "$SRC_INDEXES" "$(q "$SCRATCH_URL" "$INDEXES_Q")"
check "last migration" "$SRC_MIGRATION" "$(q "$SCRATCH_URL" "$MIGRATION_Q")"

# Every sequence at or past the highest value in the column it feeds.
SEQ_Q="SELECT s.relname || ' ' || t.relname || '.' || a.attname FROM pg_class s
  JOIN pg_depend d ON d.objid = s.oid AND d.deptype = 'a'
  JOIN pg_class t ON t.oid = d.refobjid
  JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid
  WHERE s.relkind = 'S' ORDER BY 1"
while read -r seq col; do
  [[ -z "$seq" ]] && continue
  tbl="${col%%.*}"; c="${col#*.}"
  ok="$(q "$SCRATCH_URL" "SELECT (SELECT coalesce(max(\"$c\"), 0) FROM public.\"$tbl\") <= (SELECT CASE WHEN is_called THEN last_value ELSE last_value - 1 END FROM public.\"$seq\")")"
  check "sequence $seq at or past max($col)" "t" "$ok"
done < <(q "$SCRATCH_URL" "$SEQ_Q")

# The data's own rules, in the copy (where a table isn't there, skipped).
rule() { # name, query that counts what breaks the rule
  local n
  if n="$(q "$SCRATCH_URL" "$2" 2>/dev/null)"; then check "$1" "0" "$n"; else printf '  skip  %s\n' "$1"; fi
}
rule "every app has its owner" \
  "SELECT count(*) FROM apps a LEFT JOIN accounts o ON o.id = a.owner_id WHERE o.id IS NULL"
rule "every key, origin and issuer has its app" \
  "SELECT (SELECT count(*) FROM app_keys k LEFT JOIN apps a ON a.id = k.app_id WHERE a.id IS NULL)
        + (SELECT count(*) FROM app_origins o LEFT JOIN apps a ON a.id = o.app_id WHERE a.id IS NULL)
        + (SELECT count(*) FROM app_issuers i LEFT JOIN apps a ON a.id = i.app_id WHERE a.id IS NULL)"
rule "every identity has its account" \
  "SELECT count(*) FROM identities i LEFT JOIN accounts a ON a.id = i.account_id WHERE a.id IS NULL"
rule "every recent meet room is a room" \
  "SELECT count(*) FROM meet_recent r LEFT JOIN meet_rooms m ON m.slug = r.slug WHERE m.slug IS NULL"
rule "each usage day's active users = the members with that day's bit" \
  "SELECT count(*) FROM usage_days d WHERE d.active_users <> (SELECT count(*) FROM usage_members m
     WHERE m.app_id = d.app_id AND m.month = date_trunc('month', d.day)::date
       AND (m.days & (1 << (extract(day FROM d.day)::int - 1))) <> 0)"
rule "no message is newer than now" "SELECT count(*) FROM messages WHERE inserted_at > now() + interval '5 minutes'"

# 4. cleanup (the trap) drops the scratch database.
if [[ "$fails" == 0 ]]; then
  echo "restore drill: PASSED (${#TABLES[@]} tables)"
else
  echo "restore drill: FAILED ($fails checks)"
  exit 1
fi
