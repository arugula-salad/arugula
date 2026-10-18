"""table.py CONTROL_DB TIMELINE_JSON RIG_JSON: each entry of the log as
control has it (its number, and the device that appended it) beside what a
member machine reads in it (who, and the text). Control has only the first
half. Run with python3 -I."""

import json
import sqlite3
import sys

con = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
msgs = {m["id"]: m for m in json.load(open(sys.argv[2]))["messages"]}
rig = json.load(open(sys.argv[3]))
names = {m["id"]: name for name, m in rig["machines"].items()}
print(f"{'seq':>3}  {'appended by':<11} {'who':<8} text")
for seq, device in con.execute("SELECT seq, device FROM conv_log ORDER BY seq"):
    m = msgs.get(seq, {})
    text = " ".join(m.get("text", "?").split())
    print(f"{seq:>3}  {names.get(device, device):<11} {m.get('name', '?'):<8} {text[:90]}")
