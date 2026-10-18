#!/usr/bin/env python3
"""A local ACP adapter for dev-fixture: reads its sample repo, never calls an LLM.

This makes a real agent block with deterministic content and no credentials or
API costs. The adapter identifies itself as a fixture in the UI.
"""
import json
import subprocess
import sys

folder = None
session = "visual-fixture"


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(value):
    send({"method": "session/update", "params": {"sessionId": session, "update": value}})


for line in sys.stdin:
    request = json.loads(line)
    method, ident = request.get("method"), request.get("id")
    params = request.get("params") or {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {},
                  "agentInfo": {"name": "Fixture agent", "version": "1"}, "authMethods": []}
    elif method == "session/new":
        folder = params["cwd"]
        result = {"sessionId": session}
    elif method == "session/prompt":
        prompt = "".join(p.get("text", "") for p in params.get("prompt", []))
        update({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": prompt}})
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text":
                "## Workspace review\n\nThis is the local visual fixture. I’ll inspect the sample change so the editor, diff and conversation can be reviewed together.\n\n"}})
        update({"sessionUpdate": "tool_call", "toolCallId": "fixture-diff", "title": "git diff --stat",
                "kind": "read", "status": "in_progress"})
        diff = subprocess.run(["git", "diff", "--stat"], cwd=folder, capture_output=True, text=True, check=True).stdout
        update({"sessionUpdate": "tool_call_update", "toolCallId": "fixture-diff", "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": diff}}]})
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text":
                "The sample has a working-tree change ready for review.\n\n- **Editor:** inspect the source.\n- **Diff:** compare the new content.\n- **Preview:** check the rendered page.\n\nNo model was contacted and no project files were changed by this adapter."}})
        result = {"stopReason": "end_turn"}
    elif ident is None:
        continue
    else:
        send({"id": ident, "error": {"code": -32601, "message": "Unsupported by fixture adapter"}})
        continue
    send({"id": ident, "result": result})
