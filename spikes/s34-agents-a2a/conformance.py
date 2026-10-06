"""S34: the official A2A SDK (a2a-sdk, Python) against an illogical daemon.

Parses the agent card and every answer with the SDK's protobuf types
(strict: an unknown or misspelt field fails), over the daemon's own HTTP
port with its local token: the owner's path, so no consent waits.

    python3 conformance.py BASE_URL TOKEN AGENT
"""

import asyncio
import json
import sys
import time
import uuid

import httpx
from google.protobuf import json_format

from a2a.client.transports.jsonrpc import JsonRpcTransport
from a2a.types.a2a_pb2 import (
    AgentCard,
    CancelTaskRequest,
    GetTaskRequest,
    ListTasksRequest,
    Message,
    Part,
    Role,
    SendMessageConfiguration,
    SendMessageRequest,
    TaskState,
)

base, token, agent = sys.argv[1:4]
ok = True


def check(what, cond, detail=""):
    global ok
    print(f"{'ok  ' if cond else 'FAIL'} {what}{': ' + str(detail) if detail else ''}")
    ok = ok and bool(cond)


async def main():
    headers = {"Authorization": f"Bearer {token}", "A2A-Version": "1.0"}
    async with httpx.AsyncClient(headers=headers, timeout=300) as http:
        raw = (await http.get(f"{base}/api/a2a/agents/{agent}/card")).json()
        try:
            card = json_format.ParseDict(raw, AgentCard())
            check("the agent card parses strictly as an A2A 1.0 AgentCard", True, card.name)
        except json_format.ParseError as e:
            check("the agent card parses strictly as an A2A 1.0 AgentCard", False, e)
            return
        check("its interface is JSON-RPC on illogical://", card.supported_interfaces[0].protocol_binding == "JSONRPC", card.supported_interfaces[0].url)
        t = JsonRpcTransport(http, card, f"{base}/api/a2a/agents/{agent}")

        def msg(text):
            return Message(message_id=str(uuid.uuid4()), role=Role.ROLE_USER, parts=[Part(text=text)])

        t0 = time.time()
        r = await t.send_message(
            SendMessageRequest(
                message=msg("Reply with exactly one word: pong. Don't use any tools."),
                configuration=SendMessageConfiguration(return_immediately=False),
            )
        )
        task = r.task
        check(
            "a blocking SendMessage comes back completed",
            task.status.state == TaskState.TASK_STATE_COMPLETED,
            f"{TaskState.Name(task.status.state)} in {time.time() - t0:.1f}s",
        )
        reply = [a for a in task.artifacts if a.name == "reply"]
        check("... with the reply as a text artifact", reply and "pong" in reply[0].parts[0].text.lower(), reply and reply[0].parts[0].text)

        got = await t.get_task(GetTaskRequest(id=task.id, history_length=5))
        check("GetTask parses, with history", got.id == task.id and len(got.history) >= 2, len(got.history))

        r2 = await t.send_message(
            SendMessageRequest(
                message=msg("Run `sleep 30` with Bash, then say done."),
                configuration=SendMessageConfiguration(return_immediately=True),
            )
        )
        check("returnImmediately gives a task not yet ended", r2.task.status.state in (TaskState.TASK_STATE_SUBMITTED, TaskState.TASK_STATE_WORKING), TaskState.Name(r2.task.status.state))
        c = await t.cancel_task(CancelTaskRequest(id=r2.task.id))
        check("CancelTask cancels it", c.status.state == TaskState.TASK_STATE_CANCELED)

        lst = await t.list_tasks(ListTasksRequest())
        check("ListTasks parses", len(lst.tasks) >= 2, len(lst.tasks))

        try:
            await t.get_task(GetTaskRequest(id="nope"))
            check("an unknown task is TaskNotFoundError", False)
        except Exception as e:  # noqa: BLE001
            check("an unknown task is TaskNotFoundError", "not found" in str(e).lower() or "-32001" in str(e) or "TaskNotFound" in type(e).__name__, f"{type(e).__name__}: {e}")

        bare = await http.post(
            f"{base}/api/a2a/agents/{agent}",
            headers={"A2A-Version": ""},
            json={"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": task.id}},
        )
        check("no version means 0.3, refused with VersionNotSupportedError", bare.json().get("error", {}).get("code") == -32009, json.dumps(bare.json())[:120])


asyncio.run(main())
sys.exit(0 if ok else 1)
