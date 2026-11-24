// New, in every place's heading (#1257, Arugula #1362), as the iPhone's
// New.swift: tell a board, or start a New idea. What it opens, it opens on
// the Overview.
//
// - Tell a board: pick one; its thread opens, and a post there reaches the
//   session that leads it.
// - New idea: what you say becomes a native issue, "Idea: …", whose body
//   holds the agent's instructions and your words. Then `POST
//   /api/issues/{issue}/agent` starts a hosted session on it; its
//   conversation is that session's thread, and the Overview lists it under New ideas.
//   Where the account has no hosted sessions, the idea runs as an agent block
//   on a machine you pick, and the conversation is the issue's thread.

package io.arugula.app

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.background
import kotlinx.coroutines.launch
import org.json.JSONObject

/** What the agent of a New idea is asked to do, before your words. */
fun ideaInstructions(by: String) = """
    A new idea from $by, on their phone. Their words are below.

    Ask at most two short questions here, in this thread, if you need to. Then write a project Doc that sets the idea out (the arugula doc tool, in this project), with a prototype section when one fits, and post its link here. When $by says so, turn the Doc into a board with its rows.
""".trimIndent()

/** An idea's title: its first line, cut short. */
fun ideaTitle(words: String): String {
    val t = words.lineSequence().firstOrNull()?.trim() ?: words
    return "Idea: " + if (t.length > 70) t.take(69) + "…" else t
}

private enum class Step { CHOOSE, BOARD, IDEA }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewSheet(api: Api, model: HomeModel, fleet: HomeFleet, onDismiss: () -> Unit, toast: (String) -> Unit, made: () -> Unit) {
    val nav = LocalNav.current
    val scope = LocalScope.current
    val coroutines = rememberCoroutineScope()
    var step by remember { mutableStateOf(Step.CHOOSE) }
    var words by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    /** The idea's issue, when hosted sessions weren't there for it: it runs on a machine instead. */
    var unhosted by remember { mutableStateOf<Issue?>(null) }

    fun opened(room: ChatRoom, parent: Long?, title: String) {
        onDismiss()
        made()
        nav.open(AppTab.OVERVIEW, Dest.Room(room, thread = parent, title = title))
    }

    /** File the idea, then ask Arugula cloud's agent to take it. */
    fun start() = coroutines.launch {
        val p = scope.current ?: return@launch
        busy = true
        val text = words.trim()
        val me = scope.me?.name ?: "the person"
        val issue = try {
            Issue.of(api.post("/api/projects/${p.id}/issues", JSONObject().put("title", ideaTitle(text)).put("body", ideaInstructions(me) + "\n\n---\n\n" + text)))
        } catch (e: Exception) {
            problem = describe(e)
            busy = false
            return@launch
        }
        try {
            val a = api.post("/api/issues/${issue.id}/agent")
            if (a.str("session") == null) {
                // The thread says why (no box, no repo): try a machine.
                problem = null
                unhosted = issue
            } else {
                toast("Started: ${issue.title}")
                opened(ChatRoom.thread(a.getString("room"), issue.title), a.optLong("seq"), issue.title)
            }
        } catch (e: Exception) {
            problem = describe(e)
            unhosted = issue
        }
        busy = false
    }

    /** The idea as an agent block on a machine, working on its issue; its conversation is the issue's thread. */
    fun onMachine(issue: Issue, m: MachineSession) = coroutines.launch {
        val p = scope.current ?: return@launch
        busy = true
        try {
            val prompt = "${issue.body}\n\nThis is Arugula issue #${issue.number} (${issue.id}) in project ${p.id}. Talk with them in its thread (post_thread with room ${issue.id}, and read_thread there for their answers)."
            val config = JSONObject().put("agent", "claude").put("prompt", prompt).put("title", issue.title)
                .put("work", JSONObject().put("issue", "#${issue.number}").put("project", p.id))
            m.request("POST", "/api/blocks", JSONObject().put("type", "agent").put("config", config))
            toast("Started on ${m.machine.name}: ${issue.title}")
            opened(api.threadOf(issue.id), null, issue.title)
        } catch (e: Exception) {
            problem = describe(e)
        }
        busy = false
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    when (step) {
                        Step.CHOOSE -> "New"
                        Step.BOARD -> "Tell a board"
                        Step.IDEA -> "New idea"
                    },
                    Modifier.weight(1f), style = Face.display(26.sp), color = Palette.fg,
                )
                TextLink("Cancel", onDismiss)
            }
            when (step) {
                Step.CHOOSE -> {
                    Choice("Tell a board", "Posts in its thread; the session leading it picks it up", "new-board") { step = Step.BOARD }
                    Choice("New idea", "An agent asks a question or two and writes a Doc", "new-idea") { step = Step.IDEA }
                }
                // The open boards, those with a lead first: a post on one's thread reaches its lead.
                Step.BOARD -> {
                    if (model.boards.isEmpty()) Caps("No open boards")
                    for (b in model.boards.sortedWith(compareBy({ if (it.lead == null) 1 else 0 }, { it.title }))) {
                        Choice(b.title, leadWords(b.lead), "tell-${b.title}") {
                            onDismiss()
                            nav.open(AppTab.OVERVIEW, Dest.BoardThread(b))
                        }
                    }
                }
                Step.IDEA -> {
                    val issue = unhosted
                    if (issue != null) {
                        val online = fleet.machines.filter { it.connected }
                        Text("Arugula cloud can't run it for you${problem?.let { ": $it" } ?: ""}. Run it on one of your machines?", style = Face.ui(14.sp), color = Palette.subtle)
                        if (online.isEmpty()) Caps("No machine is online")
                        for (m in online) Choice(m.machine.name, "A Claude Code agent block there, on #${issue.number}", "idea-on-${m.machine.name}") { if (!busy) onMachine(issue, m) }
                    } else {
                        // The keyboard's mic dictates into it.
                        OutlinedTextField(
                            words, { words = it }, Modifier.fillMaxWidth().heightIn(min = 120.dp).testTag("idea-words"),
                            placeholder = { Text("Say it however it comes out…") }, textStyle = Face.ui(16.sp), minLines = 4, maxLines = 10,
                        )
                        problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger) }
                        AnswerButton(if (busy) "Starting…" else "Start", Modifier.testTag("idea-start"), primary = true, enabled = !busy && words.isNotBlank()) { start() }
                        Text("An agent in Arugula cloud reads it, may ask you a question or two, then writes a Doc.", style = Face.ui(12.sp), color = Palette.dim)
                    }
                }
            }
            Spacer(Modifier.padding(4.dp))
        }
    }
}

@Composable
private fun Choice(title: String, says: String, tag: String, go: () -> Unit) {
    val shape = RoundedCornerShape(12.dp)
    Column(
        Modifier.fillMaxWidth().clip(shape).background(Palette.surface).border(1.dp, Palette.raised, shape).clickable(onClick = go).padding(12.dp).testTag(tag),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Text(title, style = Face.ui(16.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Text(says, style = Face.ui(13.sp), color = Palette.dim)
    }
}

/** A New idea on home: its title, and where its agent is. */
data class IdeaLine(val issue: Issue, val session: SessionRow?) {
    val says: String
        get() {
            val s = session ?: return "on a machine, or not started"
            if (s.waiting != null) return s.question ?: "asks you"
            return when (s.state) {
                "queued", "starting" -> "starting…"
                "working", "running" -> "writing…"
                "failed", "error" -> "failed"
                "stopped" -> "stopped"
                else -> "replied"
            }
        }

    val standing: Standing
        get() {
            val s = session ?: return Standing.IDLE
            return if (s.waiting != null) Standing.ASKS else s.standing
        }
}

@Composable
fun IdeaRow(line: IdeaLine, modifier: Modifier) {
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            Dot(line.standing.tone)
            Text(line.issue.title.removePrefix("Idea: "), style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        Text(line.says, style = Face.mono(11.sp), color = Palette.dim, maxLines = 2)
    }
}
