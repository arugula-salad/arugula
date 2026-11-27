// Answering in the row (#1256), as the iPhone's Answer.swift: what needs you
// is answered where it is on home, and the row folds away. A machine pane's
// question or approval goes to its daemon over the link (`POST
// /api/attention/act`); a hosted session's to control (`POST
// /api/sessions/{ses}/actions`); a board row's ask to its board. A session's
// peek is a sheet: its last lines, a reply to its agent, Silence, Stop (asked
// first) and its terminal or thread. A viewer reads, with no buttons.

package io.arugula.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/** The answer under a row of Needs you, as its kind takes one. [done] says what was done, and the row folds away. */
@Composable
fun NeedAnswer(need: Need, api: Api, model: HomeModel, done: (String) -> Unit) {
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val submit: (suspend () -> String) -> Unit = { send ->
        busy = true
        problem = null
        scope.launch {
            try {
                done(send())
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = describe(e)
            }
            busy = false
        }
    }
    Column(Modifier.alpha(if (busy) 0.5f else 1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        when (val t = need.target) {
            is Need.Target.Pane -> {
                val ask = t.p.asking
                when {
                    t.m.role(t.p) == "viewer" -> Caps("View only")
                    ask != null && ask.approve -> Choices(listOf("Allow", "Deny"), !busy) { c ->
                        submit {
                            act(t.m, t.p, JSONObject().put("action", if (c == "Allow") "allow" else "deny").put("id", ask.id))
                            if (c == "Allow") "Allowed" else "Denied"
                        }
                    }
                    ask != null && ask.questions.isNotEmpty() -> Questions(ask.questions, !busy) { content ->
                        submit {
                            act(t.m, t.p, JSONObject().put("action", "answer").put("id", ask.id).put("content", content))
                            content.optString("question_0").takeIf { ask.questions.size == 1 && it.isNotEmpty() } ?: "Answered"
                        }
                    }
                    t.p.failed -> Choices(listOf("Dismiss"), !busy) {
                        submit {
                            act(t.m, t.p, JSONObject().put("action", "dismiss"))
                            "Dismissed"
                        }
                    }
                }
            }
            is Need.Target.OnBoard -> {
                val a = t.row?.ask
                if (a != null && a.canAnswer != false) {
                    val answer: (JSONObject) -> suspend () -> Unit = { body -> { api.post("/api/boards/${t.board.id}/asks/${a.id}/answer", body) } }
                    if (a.options.isEmpty()) {
                        Own("Your answer", !busy) { text -> submit { answer(JSONObject().put("text", text))(); "Answered" } }
                    } else {
                        Choices(a.options, !busy) { c -> submit { answer(JSONObject().put("option", c))(); c } }
                    }
                }
            }
            // Approved with a passkey on the platform's page, where the row opens.
            is Need.Target.Deploy, is Need.Target.Todo -> {}
            is Need.Target.Session -> {
                val v = model.opened[t.s.session]
                when {
                    v == null -> {}
                    !v.may -> Caps("View only")
                    v.answered -> Caps("Answered: waiting for its box to take it")
                    !v.open -> {}
                    else -> Hosted(v, !busy) { body -> submit { api.post("/api/sessions/${t.s.session}/actions", body.first); body.second } }
                }
            }
        }
        problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger, maxLines = 3) }
    }
}

/** A hosted session's question or permission, as its card answers it: the body to send, and what to say. */
@Composable
private fun Hosted(v: SessionView, enabled: Boolean, send: (Pair<JSONObject, String>) -> Unit) {
    fun answer(text: String) = JSONObject().put("kind", "answer").put("answer", JSONObject().put("question_0", text)).also { a -> v.request?.let { a.put("request", it) } }
    when {
        v.ask == "permission" -> {
            val ids = v.options.ifEmpty { listOf("allow", "deny") }
            Choices(ids.map(::optionLabel), enabled) { label ->
                val id = ids[ids.map(::optionLabel).indexOf(label).coerceAtLeast(0)]
                val refuses = id.lowercase().startsWith("reject") || id.lowercase().startsWith("deny")
                val a = JSONObject().put("kind", if (refuses) "deny" else "approve")
                v.request?.let { a.put("request", it) }
                if (!refuses && v.options.isNotEmpty()) a.put("option", id)
                send(a to label)
            }
        }
        v.options.isNotEmpty() -> Choices(v.options, enabled) { c -> send(answer(c) to c) }
        else -> Own("Your answer", enabled) { text -> send(answer(text) to "Answered") }
    }
}

/** A permission's option id in words: `allow_always` is "Allow always". */
fun optionLabel(id: String): String {
    val w = id.replace(Regex("[-_]+"), " ").trim()
    return if (w.isEmpty()) id else w.replaceFirstChar { it.uppercase() }
}

/** A button per choice, the first one prominent. */
@Composable
internal fun Choices(all: List<String>, enabled: Boolean, pick: (String) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        all.forEachIndexed { i, c -> AnswerButton(c, Modifier.testTag("answer-$c"), primary = i == 0, enabled = enabled) { pick(c) } }
    }
}

/** The asker's own words, sent with Send. */
@Composable
internal fun Own(prompt: String, enabled: Boolean, send: (String) -> Unit) {
    var words by remember { mutableStateOf("") }
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        OutlinedTextField(words, { words = it }, Modifier.weight(1f).testTag("answer-words"), placeholder = { Text(prompt) }, singleLine = true, textStyle = Face.ui(14.sp))
        AnswerButton("Send", Modifier.padding(top = 10.dp).testTag("answer-send"), primary = true, enabled = enabled && words.isNotBlank()) { send(words.trim()) }
    }
}

/** A pane's questions: one with one pick answers at the tap; several, or a multiple choice, are picked and then sent. */
@Composable
private fun Questions(qs: List<PaneRow.Question>, enabled: Boolean, send: (JSONObject) -> Unit) {
    if (qs.size == 1 && !qs[0].multiSelect) {
        Choices(qs[0].options.map { it.label }, enabled) { c -> send(JSONObject().put("question_0", c)) }
        return
    }
    val picks = remember { mutableStateMapOf<Int, String>() }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        qs.forEachIndexed { n, q ->
            Text(q.header ?: q.question, style = Face.mono(11.sp, FontWeight.Bold), color = Palette.dim)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (o in q.options) AnswerButton(o.label, primary = picks[n] == o.label, enabled = enabled) { picks[n] = o.label }
            }
        }
        AnswerButton("Send", Modifier.testTag("answer-send"), primary = true, enabled = enabled && picks.size == qs.size) {
            val content = JSONObject()
            for ((n, c) in picks) content.put("question_$n", if (qs[n].multiSelect) JSONArray().put(c) else c)
            send(content)
        }
    }
}

internal suspend fun act(m: MachineSession, p: PaneRow, body: JSONObject) {
    body.put("pane", p.id)
    val got = m.request("POST", "/api/attention/act", body) as? JSONObject
    got?.optJSONArray("results").objects().firstOrNull { it.has("ok") && !it.optBoolean("ok") }?.let {
        throw IllegalStateException(it.str("error") ?: "the machine didn't take it")
    }
}

// ---- a session's peek

/** A session's last lines and what can be done with it, as a sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SessionPeek(api: Api, peeked: Peeked, onDismiss: () -> Unit, toast: (String) -> Unit) {
    val nav = LocalNav.current
    var lines by remember { mutableStateOf("") }
    var reply by remember { mutableStateOf("") }
    var problem by remember { mutableStateOf<String?>(null) }
    var may by remember { mutableStateOf(false) }
    var silenced by remember { mutableStateOf(false) }
    var confirmStop by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val title = when (peeked) {
        is Peeked.Pane -> peeked.m.conversations[peeked.p.id] ?: peeked.p.name
        is Peeked.Hosted -> peeked.s.issueNumber?.let { "#$it ${peeked.s.issueTitle}" } ?: "Session"
    }

    /** Its last lines: the pane's screen, or the session's history. */
    suspend fun look() {
        try {
            when (peeked) {
                is Peeked.Pane -> {
                    may = peeked.m.role(peeked.p) != "viewer"
                    silenced = peeked.m.panes.firstOrNull { it.id == peeked.p.id }?.mentionsOff ?: peeked.p.mentionsOff
                    lines = last(peeked.m.request("GET", "/api/panes/${peeked.p.id}/capture") as? String ?: "", 24)
                }
                is Peeked.Hosted -> {
                    val v = SessionView.of(api.get("/api/sessions/${peeked.s.session}"))
                    may = v.may
                    lines = v.events.takeLast(16).joinToString("\n")
                }
            }
            problem = null
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            problem = describe(e)
        }
    }
    LaunchedEffect(peeked.id) {
        while (isActive) {
            look()
            delay(3_000)
        }
    }
    val doing: (suspend () -> Unit) -> Unit = { f ->
        scope.launch {
            try {
                f()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = describe(e)
            }
        }
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(title, style = Face.ui(17.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2)
            Text(
                lines.ifEmpty { "…" },
                Modifier.fillMaxWidth().heightIn(min = 120.dp, max = 360.dp).background(Palette.void, RoundedCornerShape(10.dp))
                    .border(1.dp, Palette.raised, RoundedCornerShape(10.dp)).verticalScroll(rememberScrollState(Int.MAX_VALUE)).padding(10.dp).testTag("peek-lines"),
                style = Face.mono(11.sp), color = Palette.subtle,
            )
            problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger) }
            if (may) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OutlinedTextField(reply, { reply = it }, Modifier.weight(1f).testTag("peek-reply"), placeholder = { Text("Reply to the agent…") }, maxLines = 4, textStyle = Face.ui(15.sp))
                    AnswerButton("Send", Modifier.padding(top = 10.dp).testTag("peek-send"), primary = true, enabled = reply.isNotBlank()) {
                        doing {
                            val text = reply.trim()
                            when (peeked) {
                                is Peeked.Pane -> peeked.m.request("POST", "/api/panes/${peeked.p.id}/send", JSONObject().put("text", text).put("enter", true))
                                is Peeked.Hosted -> api.post("/api/chat/rooms/${peeked.s.room}/messages", JSONObject().put("text", text).put("parent", peeked.s.parent))
                            }
                            reply = ""
                            toast("Sent to $title")
                            look()
                        }
                    }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                if (peeked is Peeked.Pane && peeked.m.role(peeked.p) == "owner") {
                    AnswerButton(if (silenced) "Unsilence" else "Silence", Modifier.testTag("peek-silence")) {
                        doing {
                            peeked.m.request("POST", "/api/panes/${peeked.p.id}/mentions", JSONObject().put("on", silenced))
                            silenced = !silenced
                            toast(if (silenced) "Silenced: teammates' agents can't mention it" else "Teammates' agents can mention it again")
                        }
                    }
                }
                if (may) AnswerButton("Stop", Modifier.testTag("peek-stop"), danger = true) { confirmStop = true }
                when (peeked) {
                    is Peeked.Pane -> if (peeked.p.type != "agent") {
                        AnswerButton("Open terminal", Modifier.testTag("peek-terminal")) {
                            onDismiss()
                            nav.open(AppTab.MACHINES, Dest.Terminal(peeked.m.machine.id, peeked.p.id, peeked.p.tab))
                        }
                    }
                    is Peeked.Hosted -> AnswerButton("Open thread", Modifier.testTag("peek-thread")) {
                        onDismiss()
                        nav.push(Dest.Room(peeked.s.thread, thread = peeked.s.parent))
                    }
                }
                AnswerButton("Close", Modifier.testTag("peek-close")) { onDismiss() }
            }
        }
    }

    if (confirmStop) {
        AlertDialog(
            onDismissRequest = { confirmStop = false },
            title = { Text("Stop $title?") },
            text = {
                Text(
                    when (peeked) {
                        is Peeked.Pane -> if (peeked.p.isAgent) "Its turn ends where it is." else "Its terminal gets Escape, which stops an agent's turn."
                        is Peeked.Hosted -> "The session ends, in Arugula cloud."
                    },
                )
            },
            confirmButton = {
                TextButton({
                    confirmStop = false
                    doing {
                        when (peeked) {
                            is Peeked.Pane -> if (peeked.p.type == "agent") {
                                peeked.m.request("POST", "/api/blocks/${peeked.p.id}/call/cancel", JSONObject())
                            } else {
                                peeked.m.request("POST", "/api/panes/${peeked.p.id}/keys", JSONObject().put("keys", JSONArray().put("Escape")))
                            }
                            is Peeked.Hosted -> api.post("/api/sessions/${peeked.s.session}/stop")
                        }
                        toast("Stopped $title")
                        onDismiss()
                    }
                }, Modifier.testTag("peek-stop-confirm")) { Text("Stop", color = Palette.danger) }
            },
            dismissButton = { TextButton({ confirmStop = false }) { Text("Cancel") } },
        )
    }
}

/** The last [n] lines with something on them. */
private fun last(text: String, n: Int): String {
    val all = text.split('\n').map { it.trimEnd() }.dropLastWhile { it.isEmpty() }
    return all.takeLast(n).joinToString("\n")
}
