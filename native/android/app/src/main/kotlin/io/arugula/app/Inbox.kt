// The Inbox behind the bell (Arugula #1365), as the web's inbox page and its
// count (web/src/platform/pages/inbox.tsx, inbox-count.tsx): deploys waiting
// for your approval first, then the entries `/api/inbox` lists grouped by
// why they want you, the threads on objects with news chat doesn't list
// (New in threads, shared/chat/unread.ts), what's ready, merged or closed,
// and drafts. The bell counts what's unseen; opening an entry marks it seen,
// Dismiss marks it done. The heading's bell on a phone and the rail's on a
// tablet read the same model.

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
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import org.json.JSONObject

/** One entry of the Inbox, as `/api/inbox` lists it. [url] is GitHub's, or a path on the platform's page for a native issue or a run. */
data class InboxEntry(
    val item: String, val reason: String, val repo: String, val number: Int, val kind: String, val state: String, val title: String,
    val author: String?, val url: String, val project: String?, val room: String?, val updatedAt: Long, val seenAt: Long?, val doneAt: Long?,
) {
    /** Seen: marked so, or done (ready, merged or closed), which asks nothing of you (the web's `isSeen`). */
    val seen get() = seenAt != null || reason == "done"

    companion object {
        fun of(o: JSONObject) = InboxEntry(
            o.getString("item"), o.optString("reason"), o.optString("repo"), o.optInt("number"), o.optString("kind"), o.optString("state"),
            o.optString("title"), o.str("author")?.takeIf { it.isNotEmpty() }, o.optString("url"), o.str("project"), o.str("room"),
            o.optLong("updated_at"), o.long("seen_at"), o.long("done_at"),
        )
    }
}

/** A deploy waiting for your approval (`/api/deploys/waiting`): it opens on the platform's page, where it's approved. */
data class WaitingDeploy(val id: String, val project: String, val environment: String, val subject: String, val plannedBy: String) {
    companion object {
        fun of(o: JSONObject) = WaitingDeploy(
            o.getString("id"), o.obj("project")?.optString("name").orEmpty(), o.obj("environment")?.optString("name").orEmpty(),
            o.str("subject")?.takeIf { it.isNotEmpty() } ?: o.optString("plan"), o.optString("planned_by_name"),
        )
    }
}

/** An object's thread (an issue's, a run's) with news: chat's room, what it counts, and what it's on. */
data class ThreadNews(val room: ChatRoom, val target: String?, val counted: Int) {
    companion object {
        /** What a room adds to a count, as the web's `counted`: an object's thread what wants you, unless you asked for every message. */
        fun counted(o: JSONObject): Int {
            val unread = o.optInt("unread")
            val mentions = o.optInt("mentions")
            val kind = o.optString("kind")
            return when (o.str("level")) {
                "nothing" -> mentions
                "mentions" -> if (kind == "object") o.int("wants") ?: unread else if (kind == "channel") mentions else unread
                "all" -> unread
                else -> if (o.optBoolean("muted")) mentions else unread
            }
        }

        /**
         * New in threads, as the web's `threadNews`: each object's thread with
         * something that counts, unless an unseen mention in the Inbox already
         * points at it. Mentions first, then the most unread.
         */
        fun of(rooms: List<JSONObject>, entries: List<InboxEntry>): List<ThreadNews> {
            val covered = mutableSetOf<String>()
            for (e in entries) {
                if (e.reason !in MENTIONS || e.seenAt != null || e.doneAt != null) continue
                covered += e.item
                e.room?.let { covered += it }
            }
            return rooms.filter { it.optString("kind") == "object" }
                .mapNotNull { o -> runCatching { ThreadNews(ChatRoom.of(o), o.str("target"), counted(o)) }.getOrNull() }
                .filter { it.counted > 0 && it.room.id !in covered && (it.target == null || it.target !in covered) }
                .sortedWith(compareByDescending<ThreadNews> { it.room.mentions > 0 }.thenByDescending { it.counted }.thenBy { it.room.name })
        }

        private val MENTIONS = setOf("mentioned", "thread_mention")
    }
}

/**
 * The Inbox as it was last read, and its count on the bell (the web's
 * `useInboxCount`): the unseen entries but drafts, the threads with news and
 * the deploys waiting. Read as the app opens or comes back, when the project
 * changes, as a heading comes up, and after the sheet marks something.
 */
class InboxModel(private val api: Api) {
    var entries by mutableStateOf<List<InboxEntry>?>(null)
        private set
    var threads by mutableStateOf<List<ThreadNews>>(emptyList())
        private set
    var deploys by mutableStateOf<List<WaitingDeploy>>(emptyList())
        private set
    var problem by mutableStateOf<String?>(null)

    val count get() = (entries ?: emptyList()).count { it.reason != "draft" && !it.seen } + threads.size + deploys.size

    private var project: String? = null
    private var readAt = 0L
    private var reads = 0

    /** Marks and reads that outlive the sheet that asked for them. */
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    fun later(block: suspend InboxModel.() -> Unit) {
        scope.launch { block() }
    }

    suspend fun load(project: String? = this.project) = coroutineScope {
        this@InboxModel.project = project
        val n = ++reads
        val path = project?.let { "/api/inbox?project=${Uri.encode(it)}" } ?: "/api/inbox"
        val inbox = async { runCatching { api.get(path).list("entries", InboxEntry::of).sortedByDescending { it.updatedAt } } }
        val waiting = async { runCatching { api.get("/api/deploys/waiting").list("deploys", WaitingDeploy::of) }.getOrNull() }
        val rooms = async { runCatching { api.get("/api/chat").optJSONArray("rooms").objects() }.getOrNull() }
        val got = inbox.await()
        val d = waiting.await()
        val r = rooms.await()
        // A later read has the say.
        if (n != reads) return@coroutineScope
        readAt = System.currentTimeMillis()
        got.onSuccess { list ->
            entries = list
            problem = null
            r?.let { threads = ThreadNews.of(it, list.filter { e -> e.reason != "draft" }) }
        }.onFailure { problem = describe(it) }
        // Waiting deploys are the reader's to approve; another server may not have them, and then the count goes without.
        d?.let { deploys = it }
    }

    /** Again, unless it was read in the last few seconds: a heading coming up. */
    suspend fun refresh(project: String?) {
        if (project == this.project && System.currentTimeMillis() - readAt < 15_000) return
        load(project)
    }

    /** Mark [e] seen (opening it) or done (Dismiss), then read the Inbox again. */
    suspend fun mark(e: InboxEntry, seen: Boolean = false, done: Boolean = false) {
        try {
            val body = JSONObject().put("reason", e.reason)
            if (seen) body.put("seen", true)
            if (done) body.put("done", true)
            api.post("/api/inbox/${Uri.encode(e.item)}", body)
        } catch (x: Exception) {
            problem = describe(x)
        }
        load()
    }

    /** Every entry of [item] seen, as a link to it says (no reason marks them all). */
    suspend fun markItem(item: String) {
        runCatching { api.post("/api/inbox/${Uri.encode(item)}", JSONObject().put("seen", true)) }
        load()
    }

    /** A thread's Dismiss: its room read to its last message, as the web's. */
    suspend fun readThread(t: ThreadNews) {
        try {
            api.post("/api/chat/rooms/${t.room.id}/read", JSONObject().put("seq", t.room.last))
        } catch (x: Exception) {
            problem = describe(x)
        }
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

/**
 * Where an entry opens, as the web's `linkOf`: a native issue on its page in
 * the app, a thread's mention in its thread, a draft on Arugula's Inbox and
 * anything else (a PR or issue on GitHub, a run) in a Custom Tab. True when
 * it opened in the app.
 */
fun openEntry(context: android.content.Context, heading: Heading, nav: Nav, e: InboxEntry): Boolean {
    when {
        e.item.startsWith("iss_") -> nav.push(Dest.IssueRef(e.item))
        e.reason == "thread_mention" && e.room != null -> nav.push(Dest.Room(ChatRoom.thread(e.room, e.title), title = e.title))
        else -> {
            val url = if (e.reason == "draft") "/inbox" else e.url
            heading.inbox.later { openOnPlatform(context, heading.model.url, url) }
            return false
        }
    }
    return true
}

/** The bell: the Inbox, with its count beside it while anything wants you ("99+" past 99). */
@Composable
fun InboxBell(heading: Heading, modifier: Modifier = Modifier) {
    val n = heading.inbox.count
    Box {
        HeadingButton(Icons.Outlined.NotificationsNone, if (n > 0) "Inbox, $n" else "Inbox", modifier, onClick = heading.showInbox)
        if (n > 0) {
            Box(
                Modifier.align(Alignment.TopEnd).offset(x = 6.dp, y = (-4).dp).heightIn(min = 16.dp).widthIn(min = 16.dp)
                    .clip(RoundedCornerShape(8.dp)).background(if (heading.inbox.deploys.isNotEmpty()) Palette.warn else Palette.accent)
                    .padding(horizontal = 4.dp).clearAndSetSemantics {},
                contentAlignment = Alignment.Center,
            ) {
                Text(if (n > 99) "99+" else "$n", style = Face.mono(10.sp, FontWeight.Bold), color = Palette.void, maxLines = 1)
            }
        }
    }
}

/** The groups, most pressing first, as the web's: which reasons each holds, and its heading. New in threads goes after Assigned. */
private val GROUPS = listOf(
    "review_requested" to listOf("review_requested") to "Reviews requested",
    "changes_requested" to listOf("changes_requested") to "Changes requested",
    "checks_failed" to listOf("checks_failed") to "Checks failed",
    "run_failed" to listOf("run_failed") to "Runs failed",
    "mentioned" to listOf("mentioned", "thread_mention") to "Mentions",
    "assigned" to listOf("assigned") to "Assigned to you",
    "threads" to emptyList<String>() to "New in threads",
    "done" to listOf("done") to "Ready, merged or closed",
    "draft" to listOf("draft") to "Drafts waiting for you",
)

/** What's waiting for you, grouped as the web's Inbox: one opens (and is seen), Dismiss puts it away. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InboxSheet(heading: Heading, onDismiss: () -> Unit) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    val context = LocalContext.current
    val inbox = heading.inbox
    LaunchedEffect(scope.current?.id) { inbox.load(scope.current?.id) }

    fun open(e: InboxEntry) {
        if (!e.seen) inbox.later { mark(e, seen = true) }
        if (openEntry(context, heading, nav, e)) onDismiss()
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(bottom = 24.dp).testTag("inbox")) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Inbox", Modifier.weight(1f), style = Face.display(26.sp), color = Palette.fg)
                Box(Modifier.testTag("inbox-done")) { TextLink("Done", onDismiss) }
            }
            inbox.problem?.let { Text(it, Modifier.padding(horizontal = 16.dp, vertical = 6.dp), style = Face.mono(12.sp), color = Palette.danger, maxLines = 3) }
            val list = inbox.entries
            when {
                list == null -> if (inbox.problem == null) Caps("Looking…", Modifier.padding(16.dp))
                list.isEmpty() && inbox.threads.isEmpty() && inbox.deploys.isEmpty() ->
                    Caps("Nothing wants you right now", Modifier.padding(16.dp).testTag("inbox-empty"))
                else -> LazyColumn(Modifier.testTag("inbox-list")) {
                    if (inbox.deploys.isNotEmpty()) {
                        group("deploys", "Deploys waiting for you", inbox.deploys.size)
                        items(inbox.deploys, key = { "deploy:${it.id}" }) { d ->
                            DeployRow(d, Modifier.clickable { inbox.later { openOnPlatform(context, heading.model.url, "/deploys/${d.id}") } })
                            HorizontalDivider(color = Palette.raised)
                        }
                    }
                    for ((g, words) in GROUPS) {
                        val (id, reasons) = g
                        if (id == "threads") {
                            if (inbox.threads.isEmpty()) continue
                            group(id, words, inbox.threads.size)
                            items(inbox.threads, key = { "thread:${it.room.id}" }) { t ->
                                ThreadRow(t, Modifier.clickable {
                                    onDismiss()
                                    nav.push(Dest.Room(t.room, title = t.room.name.ifEmpty { "Thread" }))
                                }) { inbox.later { readThread(t) } }
                                HorizontalDivider(color = Palette.raised)
                            }
                            continue
                        }
                        val rows = list.filter { it.reason in reasons }
                        if (rows.isEmpty()) continue
                        group(id, words, rows.size)
                        items(rows, key = { "${it.item}:${it.reason}" }) { e ->
                            // A draft is sent or dropped on Arugula's page, not dismissed here.
                            InboxRow(e, Modifier.clickable { open(e) }, dismiss = if (e.reason == "draft") null else ({ inbox.later { mark(e, done = true) } }))
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

@Composable
private fun InboxRow(e: InboxEntry, modifier: Modifier, dismiss: (() -> Unit)?) {
    val seen = e.seen
    Entry(modifier.testTag("inbox-entry-${e.item}"), dismiss, e.item) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            Box(Modifier.size(7.dp).clip(CircleShape).background(if (seen) Palette.void else Palette.accent))
            val ref = when {
                e.kind == "run" -> e.repo
                e.number > 0 -> "${e.repo} #${e.number}"
                else -> e.repo
            }
            Text(ref, Modifier.weight(1f), style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (e.state.isNotEmpty()) StateChip(if (e.reason == "done" && e.state == "open") "ready" else e.state)
        }
        Text(e.title, style = Face.ui(15.sp, if (seen) FontWeight.Normal else FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        val kind = when (e.kind) {
            "pr" -> "PR"
            "run" -> "Run"
            else -> "Issue"
        }
        val by = e.author?.let { if (e.kind == "run") "started by $it" else "by $it" }
        Caps(listOfNotNull(kind, if (e.reason == "thread_mention") "in its thread" else null, by).joinToString(" · "))
    }
}

@Composable
private fun ThreadRow(t: ThreadNews, modifier: Modifier, dismiss: () -> Unit) {
    Entry(modifier.testTag("inbox-thread-${t.room.id}"), dismiss, t.room.id) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            Box(Modifier.size(7.dp).clip(CircleShape).background(if (t.room.mentions > 0) Palette.danger else Palette.accent))
            Text(t.room.name.ifEmpty { t.target ?: "Thread" }, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        Caps("${t.counted} new" + if (t.room.mentions > 0) " · mentions you" else "")
    }
}

@Composable
private fun DeployRow(d: WaitingDeploy, modifier: Modifier) {
    Entry(modifier.testTag("inbox-deploy-${d.id}"), null, d.id) {
        Text(listOf(d.project, d.environment).filter { it.isNotEmpty() }.joinToString(" · "), style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(d.subject.ifEmpty { "A deploy" }, style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Caps(listOfNotNull("Waits for your approval", d.plannedBy.takeIf { it.isNotEmpty() }?.let { "planned by $it" }).joinToString(" · "))
    }
}
