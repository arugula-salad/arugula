"""dump.py CONTROL_DB [WORD...]: control's database as SQL (what a copy of
it gives away), and whether any WORD is in it, or in the raw bytes of the
database and its WAL. Also the conversation tables, as control sees them.
Run with python3 -I."""

import os
import sqlite3
import sys

db = sys.argv[1]
words = [w.lower() for w in sys.argv[2:]]
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
dump = "\n".join(con.iterdump())
print(f"dump: {len(dump)} bytes of SQL, {sum(1 for _ in dump.splitlines())} lines")
for t in ("conv_heads", "conv_log", "conv_leases"):
    n = con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
    print(f"{t}: {n} rows")
print("conv_log (seq, at, device, bytes):")
for row in con.execute("SELECT seq, at, device, length(ct) FROM conv_log ORDER BY seq"):
    print("  ", *row)
print("conv_leases:", con.execute("SELECT holder, token, expires, cursor FROM conv_leases").fetchall())
raw = b""
for p in (db, db + "-wal"):
    if os.path.exists(p):
        raw += open(p, "rb").read()
bad = 0
for w in words:
    in_dump = w in dump.lower()
    in_raw = w.encode() in raw.lower()
    bad += in_dump or in_raw
    print(f"'{w}': in the SQL dump: {in_dump}; in the raw db+wal bytes: {in_raw}")
sys.exit(1 if bad else 0)
