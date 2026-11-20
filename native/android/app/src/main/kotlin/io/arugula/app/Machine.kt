// A machine, reached through control's relay over an encrypted channel (the
// core's Link): its panes, tabs and sessions, and requests to its own API.
// As the iPhone's MachineSession, the link's socket thread says when there's
// something to take, and everything else happens on the main thread.

package io.arugula.app

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.arugula.core.Link
import io.arugula.core.Term
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** A pane as the machine lists it: what runs there and whether it wants you. */
data class PaneRow(
    val id: Int,
    val tab: Int,
    val command: String?,
    val cwd: String?,
    /** idle, working, needs_input or done. */
    val attention: String?,
    val reasonKind: String?,
    val headline: String?,
    val sinceMs: Long?,
    /** What it asks, answered by its id: the reason's ask, else the pane's own card. */
    val asking: Asking?,
    val askMessage: String?,
    val askAt: Long?,
    val title: String?,
    val work: String?,
    val resumes: String?,
    val project: String?,
    val lastMs: Long?,
    /** `terminal`, or `agent` for an agent block. */
    val type: String?,
    val sessionProject: String?,
    val onIssue: String?,
    val onIssueProject: String?,
    val onBoards: List<Pair<String, String>>,
    val mentionsOff: Boolean,
) {
    data class Option(val label: String)
    data class Question(val question: String, val header: String?, val multiSelect: Boolean, val options: List<Option>)
    data class Asking(val id: String, val approve: Boolean, val questions: List<Question>)

    /** The program, without its arguments: "claude", "vim". */
    val program: String? get() = command?.split(' ')?.firstOrNull()?.substringAfterLast('/')

    /** What to call it: its program's title, else the conversation it resumes, else its program where it is. */
    val name: String
        get() {
            val cleaned = (title ?: "").dropWhile { !it.isLetterOrDigit() }.trim()
            val shellTitle = Regex("^[^\\s@]+@[^\\s:]+:").containsMatchIn(cleaned)
            if (readable(cleaned) && !shellTitle && cleaned != program && cleaned.lowercase() != "claude code") return cleaned
            resumes?.replace("Claude Code conversation ", "")?.let { if (readable(it)) return it }
            project?.let { return "${program ?: "shell"} in $it" }
            cwd?.let {
                val home = Regex("^/(home|Users)/[^/]+/?$").matches(it)
                return "${program ?: "shell"} in ${if (home) "~" else it.trimEnd('/').substringAfterLast('/').ifEmpty { "/" }}"
            }
            return program ?: "shell"
        }

    val failed get() = reasonKind == "failed" || reasonKind == "exited"

    /** It needs you: it asks, or it failed. */
    val needsYou get() = attention == "needs_input" || (attention == "done" && failed)

    /** A gate first, then the other asks, then failures. */
    val needsYouRank: Int
        get() = if (attention == "needs_input") (if (reasonKind == "gate") 0 else 1) else if (failed) 2 else 3

    val standing: Standing
        get() = when {
            needsYou -> if (needsYouRank <= 1) Standing.NEEDS else Standing.FAILED
            attention == "working" -> Standing.WORKING
            else -> Standing.QUIET
        }

    val isAgent get() = type == "agent" || work == "agent"

    /** Wants you first, then working, done and idle. */
    val order: Int
        get() = when (attention) {
            "needs_input" -> 0
            "working" -> 1
            "done" -> 2
            else -> 3
        }

    companion object {
        /** Words a person can read: not empty, and not a hex id. */
        fun readable(s: String): Boolean {
            val t = s.trim()
            if (t.isEmpty()) return false
            val hex = t.all { it in "0123456789abcdefABCDEF-" }
            return !(hex && t.length >= 6)
        }

        private fun questions(a: JSONArray?) = a.objects().map { q ->
            Question(q.optString("question"), q.str("header"), q.optBoolean("multiSelect"), q.list("options") { Option(it.optString("label")) })
        }

        fun of(o: JSONObject): PaneRow {
            val reason = o.obj("reason")
            val ask = o.obj("ask")
            val attention = o.str("attention")
            val asking = if (attention != "needs_input") null else reason?.obj("ask")?.let {
                Asking(it.getString("id"), it.optString("what") == "approve", questions(it.optJSONArray("questions")))
            } ?: ask?.let { Asking(it.getString("id"), it.optString("kind") == "permission", questions(it.optJSONArray("questions"))) }
            val project = o.opt("project").let { p -> (p as? JSONObject)?.optString("name") ?: (p as? String) }
            val on = o.obj("on_issue")
            return PaneRow(
                o.getInt("id"), o.optInt("tab"), o.str("command"), o.str("cwd"), attention, reason?.str("kind"),
                reason?.str("headline"), reason?.long("since_ms"), asking, ask?.str("message"), ask?.long("at_ms"),
                o.str("title"), o.str("work"), o.str("resumes"), project, o.long("last_ms"), o.str("type"),
                o.obj("session_project")?.str("id"), on?.str("issue"), on?.str("project"),
                on?.optJSONArray("boards").objects().map { it.optString("board") to it.optString("row") },
                o.optBoolean("mentions_off"),
            )
        }
    }
}

/** A tab as the machine lays it out: its grid, and each pane's rectangle in it. */
data class TabLayout(val id: Int, val name: String?, val cols: Int, val rows: Int, val cells: List<Cell>) {
    data class Cell(val pane: Int, val kind: String?, val x: Int, val y: Int, val cols: Int, val rows: Int)

    val terminals get() = cells.filter { (it.kind ?: "terminal") == "terminal" }.map { it.pane }
}

/** One of a machine's sessions: a named group of its tabs. */
data class MuxSession(val id: Int, val name: String, val tabs: List<Int>)

/** An open channel to one machine. */
class MachineSession(private val api: Api, val machine: Machine) {
    var panes by mutableStateOf<List<PaneRow>>(emptyList())
        private set
    var agents by mutableStateOf<List<PaneRow>>(emptyList())
        private set
    var tabs by mutableStateOf<List<TabLayout>>(emptyList())
        private set
    var sessions by mutableStateOf<List<MuxSession>>(emptyList())
        private set

    /** This person's role in each session; null when the machine says none (its owner). */
    var roles by mutableStateOf<Map<Int, String>?>(null)
        private set
    var problem by mutableStateOf<String?>(null)
    var connected by mutableStateOf(false)
        private set

    /** The Claude Code conversation running in each pane, by its title. */
    var conversations by mutableStateOf<Map<Int, String>>(emptyMap())
        private set

    /** Bumped each time the terminals have something new to draw. */
    var frames by mutableStateOf(0)
        private set
    private var link: Link? = null
    private var opening = false
    private var conversationsAt = 0L

    /** The terminals taking their panes' bytes, by pane. */
    val terms = mutableMapOf<Int, Term>()
    private val main = Handler(Looper.getMainLooper())
    private val scope = MainScope()

    suspend fun connect() {
        if (link != null || opening) return
        opening = true
        problem = null
        try {
            // Its callback comes on the core's socket threads, and can come
            // once more after close: hop to the main thread, and check there.
            var opened: Link? = null
            opened = withContext(Dispatchers.IO) {
                Link(api.control, machine.id) { main.post { if (opened != null && link === opened) pump() } }
            }
            link = opened
            connected = true
            pump()
        } catch (e: Exception) {
            problem = describe(e)
        } finally {
            opening = false
        }
    }

    fun disconnect() {
        val l = link ?: return
        link = null
        connected = false
        l.close()
    }

    /** Take what's queued: on the main thread only, one reader for the core's queue. */
    fun pump() {
        val l = link ?: return
        val ids = terms.keys.toIntArray()
        val flags = l.feed(ids, Array(ids.size) { terms[ids[it]] })
        if (flags and Link.PANES != 0) {
            l.panes?.let { s ->
                runCatching {
                    val o = JSONObject(s)
                    panes = o.list("panes", PaneRow::of)
                    agents = o.list("agents", PaneRow::of)
                    tabs = o.list("tabs") { t ->
                        TabLayout(
                            t.getInt("id"), t.str("name"), t.optInt("cols"), t.optInt("rows"),
                            t.list("cells") { TabLayout.Cell(it.getInt("pane"), it.str("kind"), it.optInt("x"), it.optInt("y"), it.optInt("cols"), it.optInt("rows")) },
                        )
                    }
                    sessions = o.list("sessions") { m -> MuxSession(m.getInt("id"), m.optString("name"), (0 until (m.optJSONArray("tabs")?.length() ?: 0)).map { m.getJSONArray("tabs").getInt(it) }) }
                    roles = o.optJSONArray("roles")?.let { a -> (0 until a.length()).associate { a.getJSONArray(it).getInt(0) to a.getJSONArray(it).getString(1) } }
                }
            }
            // A pane's conversation changes rarely: look again at most every 20 seconds.
            if (System.currentTimeMillis() - conversationsAt > 20_000) {
                conversationsAt = System.currentTimeMillis()
                scope.launch { loadConversations() }
            }
        }
        if (flags and Link.TERM != 0) frames++
        if (flags and Link.SAID != 0) l.said?.let { problem = it }
        if (flags and Link.CLOSED != 0) {
            val why = l.closed
            disconnect()
            if (why != "closed") problem = why
        }
    }

    /** One request to the machine's API over the link: its answer's body. */
    suspend fun request(method: String, path: String, body: JSONObject? = null): Any? {
        val l = link ?: throw IllegalStateException("not connected to ${machine.name}")
        val s = withContext(Dispatchers.IO) { l.request(method, path, body?.toString()) }
        return JSONObject(s).opt("body")
    }

    suspend fun loadConversations() {
        val body = runCatching { request("GET", "/api/conversations?live=1") as? JSONObject }.getOrNull() ?: return
        val titles = mutableMapOf<Int, String>()
        for (c in body.optJSONArray("conversations").objects()) {
            val pane = c.obj("live")?.int("pane") ?: c.int("block") ?: continue
            listOf(c.str("title"), c.str("last_prompt"), c.str("first_prompt")).firstOrNull { it != null && PaneRow.readable(it) }?.let { titles[pane] = it }
        }
        conversations = titles
    }

    /** This person's role in a pane's session: `owner`, `editor` or `viewer`. */
    fun role(of: PaneRow): String {
        val r = roles ?: return "owner"
        val session = sessions.firstOrNull { of.tab in it.tabs } ?: return "viewer"
        return r[session.id] ?: "viewer"
    }

    /** What to call a tab, as the web does. */
    fun name(of: TabLayout): String {
        of.name?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
        val mine = panes(of)
        mine.firstNotNullOfOrNull { conversations[it.id] }?.let { return it }
        return mine.firstOrNull()?.name ?: "Empty tab"
    }

    /** A tab's panes, the one that most wants you first. */
    fun panes(of: TabLayout) = panes.filter { it.tab == of.id }.sortedWith(compareBy({ it.order }, { -(it.lastMs ?: 0) }))

    // The terminal (TerminalScreen) drives these.
    fun attach(tab: Int, pane: Int, cols: Int, rows: Int) = link?.attach(tab, pane, cols, rows)
    fun view(tab: Int, pane: Int, cols: Int, rows: Int) = link?.view(tab, pane, cols, rows)
    fun detach(panes: IntArray) = link?.detach(panes)
    fun input(pane: Int, bytes: ByteArray) = link?.input(pane, bytes)
}

/**
 * A link to each of the account's machines that's online, kept while the app
 * runs, for what their panes are up to. Opened a few at a time.
 */
class HomeFleet(private val api: Api) {
    var machines by mutableStateOf<List<MachineSession>>(emptyList())
        private set

    suspend fun load() {
        val online = runCatching { api.get("/api/directory").list("daemons", Machine::of).filter { it.online } }.getOrNull() ?: return
        val ids = online.map { it.id }.toSet()
        machines.filter { it.machine.id !in ids }.forEach { it.disconnect() }
        val kept = machines.filter { it.machine.id in ids }.toMutableList()
        for (m in online) if (kept.none { it.machine.id == m.id }) kept += MachineSession(api, m)
        machines = kept
        reconnect()
    }

    /** Open every link that isn't, four at a time. */
    suspend fun reconnect() = coroutineScope {
        for (chunk in machines.filter { !it.connected }.chunked(4)) chunk.map { async { it.connect() } }.awaitAll()
    }

    fun find(id: String) = machines.firstOrNull { it.machine.id == id }
}
