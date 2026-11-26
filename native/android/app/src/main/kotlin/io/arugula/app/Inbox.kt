// The one Inbox behind the bell (Arugula #1365, #1435), as the web's inbox
// page and its count (web/src/platform/pages/inbox.tsx, inbox-count.tsx):
// `/api/inbox`'s `items` in the server's groups (Decide, Names you, Chat)
// and its `count`, shown as it is (Arugula #1430). Nothing is added to the
// count here: no waiting deploys, no thread news. A teammate's items fold
// under "N asks for Jo ›", never counted. An agent's ask is answered under
// its row over the machine link, an agent's card or to-do with its choice;
// opening an item marks it seen, Dismiss marks it done. Then the drafts
// waiting for you. The heading's bell on a phone and the rail's on a tablet
// read the same model, kept current by chat's live `inbox` event.

package io.arugula.app

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.NotificationsNone
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import org.json.JSONObject

/** A draft waiting for you to send, as `/api/inbox`'s `entries` still lists it (M170): sent or dropped on Arugula's page, never counted. */
data class InboxDraft(val item: String, val repo: String, val number: Int, val title: String) {
    companion object {
        fun of(o: JSONObject) = InboxDraft(o.getString("item"), o.optString("repo"), o.optInt("number"), o.optString("title"))
    }
}

/**
 * One item of the one Inbox (Arugula #1430), as `/api/inbox`'s `items` lists
 * it: what it is ([kind]), its [group], whether it's in the count, whom it
 * waits on ([forAccount], a teammate's folded), who asks, what it's about
 * and where it opens ([subject]), and how it's answered in place ([answer]).
 */
data class InboxItem(
    val id: String, val kind: String, val group: String, val counts: Boolean, val forAccount: String?, val escalated: Boolean,
    val title: String, val snippet: String?, val lines: List<String>, val from: From, val subject: Subject, val answer: Answer?,
    val since: Long, val seenAt: Long?, val doneAt: Long?,
) {
    data class From(val account: String?, val name: String?, val agent: String?, val machine: String?)

    /** [type] is pane, session, proposal, board_row, deploy, pr, issue, run, room or doc; [url] where Open goes. */
    data class Subject(
        val type: String, val id: String, val url: String, val project: String?, val machine: String?, val pane: String?,
        val room: String?, val seq: Long?, val board: String?,
    )

    /** [via]: attention_act, session_action, card, todo, board_answer, deploy or chat_reply; [options] where the server holds them. */
    data class Answer(val via: String, val options: List<String>)

    val seen get() = seenAt != null

    /** Waits on a teammate, not [me] (Arugula #1393): folded under their name, never counted. */
    fun forTeammate(me: String?) = forAccount != null && me != null && forAccount != me

    /** In the reader's count, by the server's rule. */
    fun counted(me: String?) = counts && doneAt == null && !forTeammate(me)

    companion object {
        fun of(o: JSONObject): InboxItem {
            val f = o.obj("from") ?: JSONObject()
            val s = o.getJSONObject("subject")
            return InboxItem(
                o.getString("id"), o.optString("kind"), o.optString("group"), o.optBoolean("counts"), o.str("for")?.takeIf { it.isNotEmpty() },
                o.optBoolean("escalated"), o.optString("title"), o.str("snippet")?.takeIf { it.isNotEmpty() }, o.optJSONArray("lines").strings(),
                From(f.str("account"), f.str("name")?.takeIf { it.isNotEmpty() }, f.str("agent"), f.str("machine")),
                Subject(
                    s.optString("type"), s.optString("id"), s.optString("url"), s.str("project"), s.str("machine"), s.str("pane"),
                    s.str("room"), s.long("seq"), s.str("board"),
                ),
                o.obj("answer")?.let { Answer(it.optString("via"), it.optJSONArray("options").strings()) },
                o.optLong("since"), o.long("seen_at"), o.long("done_at"),
            )
        }

        /** What an item is, in a few words, as the web's `kindWords`. A kind this doesn't know says nothing: its title says it. */
        val KINDS = mapOf(
            "agent_ask" to "An agent asks",
            "permission" to "An agent asks for a permission",
            "agent_failed" to "An agent failed",
            "approval" to "Your agent asks to go ahead",
            "todo" to "Your agent asks you to do this",
            "board_ask" to "A board asks",
            "deploy_approval" to "Waits for your approval",
            "checks_failed" to "Checks failed",
            "run_failed" to "Run failed",
            "changes_requested" to "Changes requested",
            "review_request" to "Review requested",
            "mention" to "Mentions you",
            "assigned" to "Assigned to you",
            "dm" to "Message",
            "thread_news" to "New in its thread",
        )
    }
}

/** "1 ask for Jo ›", "3 asks for Jo ›". */
fun asksFor(n: Int, name: String) = "$n ${if (n == 1) "ask" else "asks"} for $name ›"

/**
 * The one Inbox as it was last read, and the bell's count: the server's
 * (Arugula #1430), across the reader's projects, shown as it is. Nothing is
 * added to it here. Read as the app opens or comes back, as a heading comes
 * up, when the live stream says it changed (its `inbox` event, whose count
 * shows at once), and after the sheet marks or answers something.
 */
class InboxModel(private val api: Api) {
    var items by mutableStateOf<List<InboxItem>?>(null)
        private set
    var drafts by mutableStateOf<List<InboxDraft>>(emptyList())
        private set
    var count by mutableIntStateOf(0)
        private set
    var problem by mutableStateOf<String?>(null)

    private var readAt = 0L
    private var reads = 0

    /** Marks and reads that outlive the sheet that asked for them. */
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    fun later(block: suspend InboxModel.() -> Unit) {
        scope.launch { block() }
    }

    /** Whether a deploy waits for [me]'s approval among what counts: the bell says so in its colour. */
    fun deployWaits(me: String?) = items.orEmpty().any { it.kind == "deploy_approval" && it.counted(me) }

    suspend fun load() {
        val n = ++reads
        val got = runCatching { api.get("/api/inbox") }
        // A later read has the say.
        if (n != reads) return
        readAt = System.currentTimeMillis()
        got.onSuccess { o ->
            items = o.list("items", InboxItem::of)
            drafts = o.optJSONArray("entries").objects().filter { it.optString("reason") == "draft" }.mapNotNull { runCatching { InboxDraft.of(it) }.getOrNull() }
            count = o.optInt("count")
            problem = null
        }.onFailure { problem = describe(it) }
    }

    /** Again, unless it was read in the last few seconds: a heading coming up. */
    suspend fun refresh() {
        if (System.currentTimeMillis() - readAt < 15_000) return
        load()
    }

    /** The stream's word (its `inbox` event): the count now, the list with the read that follows. */
    fun heard(n: Int) {
        count = n
    }

    /** Mark [i] seen (opening it) or done (Dismiss), then read the Inbox again. */
    suspend fun mark(i: InboxItem, seen: Boolean = false, done: Boolean = false) {
        try {
            val body = JSONObject()
            if (seen) body.put("seen", true)
            if (done) body.put("done", true)
            api.post("/api/inbox/${Uri.encode(i.id)}", body)
        } catch (x: Exception) {
            problem = describe(x)
        }
        load()
    }

    /** Answer an agent's card (an `approval`, a `todo`, Arugula #1467) with one of its options, and a to-do's note, as its staff card does. */
    suspend fun choose(i: InboxItem, choice: String, note: String? = null) {
        val body = JSONObject().put("choice", choice)
        if (!note.isNullOrBlank()) body.put("note", note.trim())
        try {
            api.post("/api/inbox/${Uri.encode(i.id)}", body)
        } finally {
            load()
        }
    }

    /** Every entry of [item] seen, as a link to it says (no reason marks them all). */
    suspend fun markItem(item: String) {
        runCatching { api.post("/api/inbox/${Uri.encode(item)}", JSONObject().put("seen", true)) }
        load()
    }
}

/** Where an entry or anything else the app can't show opens: an address as it is, a path on the platform's page. */
suspend fun openOnPlatform(context: android.content.Context, control: String, url: String) {
    if (Regex("^[a-z][a-z0-9+.-]*:", RegexOption.IGNORE_CASE).containsMatchIn(url)) {
        openTab(context, url)
    } else {
        platformOrigin(control)?.let { openTab(context, it + "/" + url.trimStart('/')) }
    }
}

/** The pane an item is about (a `pane` subject, Arugula #1431), if a link to its machine has it. */
fun HomeFleet.paneOf(i: InboxItem): Pair<MachineSession, PaneRow>? {
    val s = i.subject
    if (s.type != "pane") return null
    val m = find(s.machine ?: return null) ?: return null
    val id = s.pane?.trimStart('%')?.toIntOrNull() ?: return null
    return (m.panes + m.agents).firstOrNull { it.id == id }?.let { m to it }
}

/**
 * Where an item opens, as the web's: a native issue on its page in the app,
 * a chat room (a DM, a thread, a mention in it) in chat, and anything else (a
 * PR or issue on GitHub, a run, a deploy, a pane's ask) at its url, in a
 * Custom Tab. True when it opened in the app.
 */
fun openItem(context: android.content.Context, heading: Heading, nav: Nav, i: InboxItem): Boolean {
    val s = i.subject
    when {
        s.id.startsWith("iss_") -> nav.push(Dest.IssueRef(s.id))
        s.type == "room" && s.room != null -> nav.push(Dest.Room(ChatRoom.thread(s.room, i.title), title = i.title))
        else -> {
            heading.inbox.later { openOnPlatform(context, heading.model.url, s.url) }
            return false
        }
    }
    return true
}

/** The bell: the Inbox, with the server's count beside it while anything counts ("99+" past 99), in the warning's colour while a deploy waits for you. */
@Composable
fun InboxBell(heading: Heading, modifier: Modifier = Modifier) {
    val n = heading.inbox.count
    val me = LocalScope.current.me?.account
    Box {
        HeadingButton(Icons.Outlined.NotificationsNone, if (n > 0) "Inbox, $n" else "Inbox", modifier, onClick = heading.showInbox)
        if (n > 0) {
            Box(
                Modifier.align(Alignment.TopEnd).offset(x = 6.dp, y = (-4).dp).heightIn(min = 16.dp).widthIn(min = 16.dp)
                    .clip(RoundedCornerShape(8.dp)).background(if (heading.inbox.deployWaits(me)) Palette.warn else Palette.accent)
                    .padding(horizontal = 4.dp).clearAndSetSemantics {},
                contentAlignment = Alignment.Center,
            ) {
                Text(if (n > 99) "99+" else "$n", style = Face.mono(10.sp, FontWeight.Bold), color = Palette.void, maxLines = 1)
            }
        }
    }
}

/** The server's groups, in its order, as headings. `ready` (a finished turn, never counted) isn't listed. */
private val GROUPS = listOf(
    "decide" to "Decide",
    "named" to "Names you",
    "chat" to "Chat",
)

/**
 * The one Inbox, as the server lists it, in its groups: one opens (and is
 * seen), Dismiss puts it away, and what can be answered here is answered
 * under its row. A teammate's are folded under "N asks for Jo ›". Then the
 * drafts waiting for you.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InboxSheet(heading: Heading, onDismiss: () -> Unit) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    val context = LocalContext.current
    val inbox = heading.inbox
    val me = scope.me?.account
    val unfolded = remember { mutableStateMapOf<String, Boolean>() }
    LaunchedEffect(Unit) { inbox.load() }

    fun open(i: InboxItem) {
        if (!i.seen) inbox.later { mark(i, seen = true) }
        if (openItem(context, heading, nav, i)) onDismiss()
    }

    /** Dismiss: done; a pane's ask of yours is told so too, as the attention drawer does. */
    fun dismiss(i: InboxItem) = inbox.later {
        if (!i.forTeammate(me)) heading.fleet.paneOf(i)?.let { (m, p) -> runCatching { act(m, p, JSONObject().put("action", "dismiss")) } }
        mark(i, done = true)
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(bottom = 24.dp).testTag("inbox")) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Inbox", Modifier.weight(1f), style = Face.display(26.sp), color = Palette.fg)
                Box(Modifier.testTag("inbox-done")) { TextLink("Done", onDismiss) }
            }
            inbox.problem?.let { Text(it, Modifier.padding(horizontal = 16.dp, vertical = 6.dp), style = Face.mono(12.sp), color = Palette.danger, maxLines = 3) }
            val list = inbox.items?.filter { it.group != "ready" && it.doneAt == null }
            when {
                list == null -> if (inbox.problem == null) Caps("Looking…", Modifier.padding(16.dp))
                list.none { g -> GROUPS.any { it.first == g.group } } && inbox.drafts.isEmpty() ->
                    Caps("Nothing wants you right now", Modifier.padding(16.dp).testTag("inbox-empty"))
                else -> LazyColumn(Modifier.testTag("inbox-list")) {
                    for ((id, words) in GROUPS) {
                        val rows = list.filter { it.group == id }
                        if (rows.isEmpty()) continue
                        val yours = rows.filter { !it.forTeammate(me) }
                        val folds = rows.filter { it.forTeammate(me) }.groupBy { it.forAccount!! }.entries.sortedByDescending { it.value.size }
                        group(id, words, yours.size)
                        items(yours, key = { it.id }) { i ->
                            ItemRow(i, heading, Modifier.clickable { open(i) }, answers = true) { dismiss(i) }
                            HorizontalDivider(color = Palette.raised)
                        }
                        for ((who, theirs) in folds) {
                            val key = "$id $who"
                            item(key = "for:$key") {
                                Box(Modifier.padding(horizontal = 4.dp).testTag("inbox-for-$who")) {
                                    TextLink(asksFor(theirs.size, scope.nameOf(who))) { unfolded[key] = unfolded[key] != true }
                                }
                            }
                            if (unfolded[key] == true) {
                                items(theirs, key = { "$key:${it.id}" }) { i ->
                                    // Theirs to answer: it opens, and is put away, but isn't answered here.
                                    ItemRow(i, heading, Modifier.clickable { open(i) }, answers = false) { dismiss(i) }
                                    HorizontalDivider(color = Palette.raised)
                                }
                            }
                        }
                    }
                    if (inbox.drafts.isNotEmpty()) {
                        group("draft", "Drafts waiting for you", inbox.drafts.size)
                        // A draft is sent or dropped on Arugula's page, not dismissed here.
                        items(inbox.drafts, key = { "draft:${it.item}" }) { d ->
                            DraftRow(d, Modifier.clickable { inbox.later { openOnPlatform(context, heading.model.url, "/inbox") } })
                            HorizontalDivider(color = Palette.raised)
                        }
                    }
                }
            }
        }
    }
}

/** A group's heading, with how many are in it. */
private fun LazyListScope.group(id: String, words: String, n: Int) = item(key = "group:$id") {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 4.dp).testTag("inbox-group-$id"), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Caps(words, color = Palette.subtle)
        Caps("$n")
    }
}

/** A row's frame: what it is (opening it) and Dismiss beside it. */
@Composable
private fun Entry(modifier: Modifier, dismiss: (() -> Unit)?, tag: String, body: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(modifier.weight(1f).padding(start = 16.dp, end = if (dismiss == null) 16.dp else 4.dp, top = 10.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) { body() }
        if (dismiss != null) Box(Modifier.padding(end = 8.dp).testTag("inbox-dismiss-$tag")) { TextLink("Dismiss", dismiss) }
    }
}

/** An item's title, its reference apart where it starts with one (`acme/app#7 Retry…`, `#12 Flaky…`), as the web's `titled`. */
private fun titled(title: String): Pair<String?, String> {
    val m = Regex("^(\\S*#\\d+) (.+)$").find(title) ?: return null to title
    return m.groupValues[1] to m.groupValues[2]
}

/** One item: whose or which, its title, what it says, what it is; and, when [answers], its answer under it. A kind this doesn't know is its title alone. */
@Composable
private fun ItemRow(i: InboxItem, heading: Heading, modifier: Modifier, answers: Boolean, dismiss: () -> Unit) {
    val seen = i.seen
    val (ref, title) = titled(i.title)
    Entry(modifier.testTag("inbox-item-${i.id}"), dismiss, i.id) {
        val top = listOfNotNull(ref, i.from.name).joinToString(" · ")
        if (top.isNotEmpty()) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(if (seen) Palette.void else Palette.accent))
                Text(top, Modifier.weight(1f), style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        Text(title, style = Face.ui(15.sp, if (seen) FontWeight.Normal else FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        i.snippet?.let { Text(it, style = Face.ui(14.sp), color = Palette.subtle, maxLines = 3, overflow = TextOverflow.Ellipsis) }
        for (l in i.lines) Text(l, style = Face.mono(12.sp), color = Palette.subtle, maxLines = 4, overflow = TextOverflow.Ellipsis)
        val what = listOfNotNull(InboxItem.KINDS[i.kind], if (i.escalated) "no agent answered it" else null).joinToString(" · ")
        if (what.isNotEmpty()) Caps(what)
        if (answers) InPlace(i, heading)
    }
}

/**
 * An item's answer under its row, as its `answer.via` says: a pane's ask of
 * yours over the machine link (`/api/attention/act`, as Needs you answers
 * it), an agent's card (`card`: its options) and a to-do (`todo`: Done or
 * Can't, with a note) with `POST /api/inbox/{id}`'s choice, and a board's ask
 * to its board. Anything else opens where it is.
 */
@Composable
private fun InPlace(i: InboxItem, heading: Heading) {
    val a = i.answer ?: return
    val inbox = heading.inbox
    if (a.via == "attention_act") {
        val (m, p) = heading.fleet.paneOf(i) ?: return
        if (!p.needsYou) return
        Box(Modifier.padding(top = 5.dp)) { NeedAnswer(paneNeed(m, p), heading.api, heading.home) { inbox.later { load() } } }
        return
    }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var note by remember { mutableStateOf("") }
    val coroutines = rememberCoroutineScope()
    val send: (suspend () -> Unit) -> Unit = { f ->
        busy = true
        problem = null
        coroutines.launch {
            try {
                f()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = describe(e)
            }
            busy = false
        }
    }
    Column(Modifier.padding(top = 5.dp).alpha(if (busy) 0.5f else 1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        when (a.via) {
            "card" -> Choices(a.options, !busy) { c -> send { inbox.choose(i, c) } }
            "todo" -> {
                OutlinedTextField(note, { note = it }, Modifier.fillMaxWidth().testTag("inbox-note"), placeholder = { Text("A note (optional)") }, maxLines = 3, textStyle = Face.ui(14.sp))
                Choices(a.options.ifEmpty { listOf("Done", "Can't") }, !busy) { c -> send { inbox.choose(i, c, note) } }
            }
            "board_answer" -> {
                val board = i.subject.board
                if (board != null) {
                    val path = "/api/boards/${Uri.encode(board)}/asks/${Uri.encode(i.id)}/answer"
                    val answered: suspend (JSONObject) -> Unit = { body -> heading.api.post(path, body).also { inbox.load() } }
                    if (a.options.isEmpty()) {
                        Own("Your answer", !busy) { text -> send { answered(JSONObject().put("text", text)) } }
                    } else {
                        Choices(a.options, !busy) { c -> send { answered(JSONObject().put("option", c)) } }
                    }
                }
            }
        }
        problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger, maxLines = 3) }
    }
}

@Composable
private fun DraftRow(d: InboxDraft, modifier: Modifier) {
    Entry(modifier.testTag("inbox-entry-${d.item}"), null, d.item) {
        Text(if (d.number > 0) "${d.repo} #${d.number}" else d.repo, style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(d.title, style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Caps("Draft · waits for you to send")
    }
}
