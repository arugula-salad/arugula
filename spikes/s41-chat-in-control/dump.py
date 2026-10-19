#!/usr/bin/env python3
"""dump.py DB: what control's database holds of chat after the rig ran.

Rooms, messages (plain text, by design: control reads chat now), read
marks, the pushes control sent, and a check that no pane's output reached
control except what someone quoted.
"""
import sqlite3
import sys

db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
q = lambda sql: db.execute(sql).fetchall()

print("== chat_rooms (id, scope, kind, name, machine, target, last_seq)")
for r in q("SELECT id, scope, kind, name, machine, target, last_seq FROM chat_rooms ORDER BY created"):
    print("  ", r)
print("== chat_dm_members")
for r in q("SELECT room, account FROM chat_dm_members ORDER BY room, account"):
    print("  ", r)
print("== chat_messages (room, seq, author, text, ref, quote, mentions, imported)")
for r in q("SELECT room, seq, author, text, ref_machine || '/' || ref_pane, quote, mentions, imported FROM chat_messages ORDER BY room, seq"):
    print("  ", r)
print("== chat_reads")
for r in q("SELECT room, account, seq FROM chat_reads ORDER BY room, account"):
    print("  ", r)
print("== chat_pushes (account, room, seq, why, subscriptions)")
for r in q("SELECT account, room, seq, why, subs FROM chat_pushes ORDER BY id"):
    print("  ", r)
# The pane printed "running 3 tests" too; nobody quoted that line.
raw = open(sys.argv[1], "rb").read()
for wal in (sys.argv[1] + "-wal",):
    try:
        raw += open(wal, "rb").read()
    except FileNotFoundError:
        pass
for probe in [b"running 3 tests", b"health 200", b"calc::mean ... FAILED"]:
    print(f"== {probe.decode()!r} in the database's bytes: {probe in raw}")
