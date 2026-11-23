// Home (#1255): the project's conversations, sorted by what needs you, as the
// iPhone's Home.swift and the desktop's Overview read them. Needs you first,
// merged from every machine's panes, the boards' rows and the hosted
// sessions, worst first, each answered where it is (Answer.kt); then the New
// ideas; then one row per open board, with its lane bar, its count, its lead
// and what's going on in words; then the people, its channels and DMs. A
// board's row opens the board's thread, whose Board button opens its lanes.
// New tells a board or starts a New idea (New.kt).

package io.arugula.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** The label for what needs you, as the web's (web/src/shared/needs-you.ts). */
const val NEEDS_YOU = "Needs you"

// ---- what home reads from control

class HomeModel(private val api: Api) {
    /** The project's open boards, as its list orders them. */
    var boards by mutableStateOf<List<Board>>(emptyList())
        private set

    /** Each open board's rows, by its id. */
    var views by mutableStateOf<Map<String, BoardDetail>>(emptyMap())
        private set
    var rooms by mutableStateOf<List<ChatRoom>>(emptyList())
        private set
    var hosted by mutableStateOf<List<SessionRow>>(emptyList())
        private set

    /** What each hosted session that waits on you asks, and whether you may answer. */
    var opened by mutableStateOf<Map<String, SessionView>>(emptyMap())
        private set
    var problem by mutableStateOf<String?>(null)
        private set
    var loaded by mutableStateOf(false)
        private set

    /** The project's open New ideas: its issues titled "Idea: …". */
    var ideas by mutableStateOf<List<Issue>>(emptyList())
        private set

    /** Each open board's thread, by the board's id: its changes come on it. */
    val threads = mutableStateMapOf<String, ChatRoom>()
    private var project: String? = null
    private var loadedAt = 0L
    private var pending = false

    suspend fun load(project: String) = coroutineScope {
        loadedAt = System.currentTimeMillis()
        if (project != this@HomeModel.project) {
            this@HomeModel.project = project
            boards = emptyList()
            views = emptyMap()
            hosted = emptyList()
            loaded = false
        }
        val r = async { runCatching { api.get("/api/chat").list("rooms", ChatRoom::of) }.getOrNull() }
        // A hub or an account without hosted sessions has none to wait on.
        val s = async { runCatching { api.get("/api/projects/$project/sessions").list("sessions", SessionRow::of) }.getOrDefault(emptyList()) }
        val i = async { runCatching { api.get("/api/projects/$project/issues").list("issues", Issue::of) }.getOrDefault(emptyList()) }
        try {
            val open = api.get("/api/projects/$project/boards").list("boards", Board::of).filter { it.closedAt == null }
            val read = open.map { b -> async { b.id to runCatching { BoardDetail.of(api.get("/api/boards/${b.id}")) }.getOrNull() } }.awaitAll()
            if (this@HomeModel.project != project) return@coroutineScope
            boards = open
            views = read.mapNotNull { (id, v) -> v?.let { id to it } }.toMap()
            problem = null
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            problem = describe(e)
        }
        r.await()?.let { rooms = it }
        hosted = s.await()
        opened = hosted.filter { it.waiting != null }.mapNotNull { w ->
            runCatching { w.session to SessionView.of(api.get("/api/sessions/${w.session}")) }.getOrNull()
        }.toMap()
        ideas = i.await().filter { it.title.startsWith("Idea: ") }
        loaded = true
        // Each board's thread, once, as the web's Overview opens them: a board's change comes as a change on its thread.
        for (b in boards) if (b.id !in threads) runCatching { thread(b) }
    }

    /** Load again, but no sooner than five seconds after the last: what the stream sets off in a burst comes to one load. */
    suspend fun soon(project: String) {
        if (pending) return
        pending = true
        try {
            val wait = 5_000 - (System.currentTimeMillis() - loadedAt)
            if (wait > 0) delay(wait)
        } finally {
            pending = false
        }
        load(project)
    }

    /** The board's thread, made the first time someone opens it. */
    suspend fun thread(board: Board): ChatRoom = threads[board.id] ?: api.threadOf(board.id).also { threads[board.id] = it }

    /** The board whose thread this room is. */
    fun boardOfThread(room: String) = threads.entries.firstOrNull { it.value.id == room }?.key?.let { id -> boards.firstOrNull { it.id == id } }
}

// ---- how things stand

/** What a row says of itself, besides its sessions. */
fun BoardDetail.Row.says(): List<Standing> {
    val out = mutableListOf<Standing>()
    ask?.let { if (it.mine) out += Standing.ASKS else if (!it.who.isNullOrEmpty()) out += Standing.WAITS }
    if ("stalled" in marks) out += Standing.QUIET
    val work = attachments.filter { it.role == "work" && !it.released }.mapNotNull { it.live }
    for (l in listOfNotNull(live) + work) if (l.kind == "pr") {
        when (l.state) {
            "checks_failed" -> out += Standing.CHECKS_FAILING
            "checks_running" -> out += Standing.CHECKS_RUNNING
            "open" -> out += Standing.REVIEW
        }
    }
    if (lane == "lane_review" || lane == "review") out += Standing.REVIEW
    return out
}

val BoardDetail.Row.inFlight get() = lane in listOf("doing", "review", "lane_doing", "lane_review")

/** `#983 · Its title`, or its note's first line. */
val BoardDetail.Row.words: String
    get() {
        val label = live?.let { l -> l.number?.takeIf { it > 0 }?.let { n -> if (l.kind == "native") "#$n" else "${l.repo ?: ""}#$n" } }
        val title = live?.title ?: note?.lineSequence()?.firstOrNull() ?: ""
        return listOfNotNull(label, title).filter { it.isNotEmpty() }.joinToString(" · ").ifEmpty { "A card" }
    }

/** A session to peek at: a pane on a machine, or a hosted session. */
sealed interface Peeked {
    val id: String

    data class Pane(val m: MachineSession, val p: PaneRow) : Peeked {
        override val id get() = "pane:${m.machine.id}:${p.id}"
    }

    data class Hosted(val s: SessionRow) : Peeked {
        override val id get() = "hosted:${s.session}"
    }
}

/** One thing that needs you, as a row of home's first section. */
data class Need(
    val id: String,
    /** What it asks, exactly: answered, the row folds away until it asks something else. */
    val ask: String,
    val rank: Int,
    val since: Long,
    /** Who or what: an agent, a board's asker, a PR. */
    val who: String,
    val standing: Standing,
    /** Where: its machine or board, and its card. */
    val on: String,
    val text: String,
    val mono: Boolean,
    val target: Target,
) {
    sealed interface Target {
        data class Pane(val m: MachineSession, val p: PaneRow) : Target
        data class OnBoard(val board: Board, val row: BoardDetail.Row?) : Target
        data class Session(val s: SessionRow) : Target
    }
}

/** A session working on one of a board's cards, as its thread lists it. */
/** [asks]: whom it waits on, when that's someone else ([Standing.WAITS]). */
data class BoardSession(val peeked: Peeked, val name: String, val card: String, val standing: Standing, val asks: String? = null) {
    val word get() = if (standing == Standing.IDLE) "quiet" else standing.word(asks)
}

/** How a pane stands for the reader: one on someone else's machine that needs them waits on them, not on you (Arugula #1392). */
fun MachineSession.standing(p: PaneRow): Standing = if (p.standing == Standing.NEEDS && machine.ownerName != null) Standing.WAITS else p.standing

/** One open board as home's row reads it. */
data class BoardLine(
    val board: Board, val done: Int, val review: Int, val doing: Int, val total: Int, val standing: Standing,
    /** What's going on, the worst first: `1 needs you · 2 working`. */
    val says: List<Pair<Standing, String>>,
    val still: Boolean,
)

/** What home shows, from what the machines, the boards and the hosted sessions say: worked out each time any of them moves. */
class HomeState(project: Project?, fleet: HomeFleet, model: HomeModel) {
    val needs = mutableListOf<Need>()
    val boards = mutableListOf<BoardLine>()
    val sessions = mutableMapOf<String, List<BoardSession>>()

    init {
        if (project != null) build(project, fleet, model)
    }

    private fun build(project: Project, fleet: HomeFleet, model: HomeModel) {
        val personal = project.team == null
        // The project's panes: its sessions', its cards', or none's in the personal project.
        val panes = fleet.machines.flatMap { m ->
            (m.panes + m.agents).filter { p ->
                p.sessionProject == project.id || p.onIssueProject == project.id ||
                    (personal && p.sessionProject == null && p.onIssueProject == null)
            }.map { m to it }
        }

        for ((m, p) in panes) if (p.needsYou) {
            val card = p.onIssue?.let { " · $it" } ?: ""
            needs += Need(
                "pane:${m.machine.id}:${p.id}", p.asking?.id ?: "${p.reasonKind}:${p.sinceMs}", p.needsYouRank,
                p.sinceMs ?: p.askAt ?: 0, m.conversations[p.id] ?: p.name, p.standing, m.machine.name + card,
                p.headline?.takeIf { it.isNotEmpty() } ?: p.askMessage ?: "", p.asking?.approve == true || p.failed,
                Need.Target.Pane(m, p),
            )
        }

        for (b in model.boards) {
            val v = model.views[b.id] ?: continue
            for (r in v.lanes.flatMap { it.rows }) {
                if ((r.lane ?: "").endsWith("done")) continue
                val where = "${b.title} · ${r.words}"
                val a = r.ask
                if (a != null && a.mine) {
                    needs += Need(
                        "ask:${b.id}:${r.id}", a.id, 1, a.createdAt ?: 0, a.askedBy?.takeIf { it.isNotEmpty() } ?: "The board",
                        Standing.ASKS, where, a.question, false, Need.Target.OnBoard(b, r),
                    )
                } else {
                    val worst = r.says().minOrNull()
                    if (worst == Standing.CHECKS_FAILING || worst == Standing.REVIEW) {
                        needs += Need(
                            "row:${b.id}:${r.id}", worst.name, if (worst == Standing.CHECKS_FAILING) 2 else 3, r.updatedAt ?: 0, r.words, worst,
                            b.title, if (worst == Standing.CHECKS_FAILING) "Its checks fail." else "It's ready for review.", false, Need.Target.OnBoard(b, r),
                        )
                    }
                }
            }
        }

        for (s in model.hosted) {
            val waiting = s.waiting ?: continue
            val asks = "question" in waiting || "permission" in waiting
            needs += Need(
                "hosted:${s.session}", model.opened[s.session]?.request ?: waiting, if (asks) 1 else 2, s.updatedAt,
                s.issueNumber?.let { "#$it ${s.issueTitle}" } ?: "A session", Standing.NEEDS, "Arugula cloud",
                s.question ?: waiting, false, Need.Target.Session(s),
            )
        }
        needs.sortWith(compareBy({ it.rank }, { it.since }, { it.id }))

        for (b in model.boards) {
            val rows = model.views[b.id]?.lanes?.flatMap { it.rows } ?: emptyList()
            val on = mutableListOf<BoardSession>()
            val flight = rows.filter { it.inFlight }.map { r ->
                val all = r.says().toMutableList()
                for ((m, p) in panes) if (works(r, b.id, m.machine.id, p)) {
                    all += m.standing(p)
                    on += BoardSession(Peeked.Pane(m, p), m.conversations[p.id] ?: p.name, r.words, m.standing(p), m.machine.ownerName)
                }
                for (s in model.hosted) if (r.pointer?.kind == "issue" && s.issueId == r.pointer.id) {
                    all += s.standing
                    on += BoardSession(Peeked.Hosted(s), "Arugula cloud", r.words, s.standing)
                }
                all.minOrNull() ?: Standing.IDLE
            }
            sessions[b.id] = on.sortedWith(compareBy({ it.standing }, { it.name }))
            var says = listOf(Standing.NEEDS, Standing.ASKS, Standing.FAILED, Standing.CHECKS_FAILING, Standing.WAITS, Standing.QUIET, Standing.WORKING, Standing.CHECKS_RUNNING, Standing.REVIEW)
                .map { st -> st to flight.count { it == st } }.filter { it.second > 0 }.map { (st, n) -> st to "$n ${st.word}" }
            val idle = flight.count { it == Standing.IDLE }
            if (flight.isEmpty()) {
                says = listOf(Standing.IDLE to "nothing in flight")
            } else if (idle > 0) {
                says = says + (Standing.IDLE to if (says.isEmpty()) "$idle in flight" else "$idle more in flight")
            }
            val p = b.progress
            boards += BoardLine(b, p?.done ?: 0, p?.review ?: 0, p?.doing ?: 0, p?.total ?: 0, flight.minOrNull() ?: Standing.IDLE, says, flight.isEmpty())
        }
        boards.sortWith(compareBy({ if (it.still) 1 else 0 }, { it.standing }, { it.board.title.lowercase() }))
    }

    /** Whether a pane works on a row: its agent's card, or the row's attachment of its block. */
    private fun works(r: BoardDetail.Row, board: String, machine: String, p: PaneRow): Boolean {
        p.onIssue?.let { issue ->
            if (p.onBoards.any { it.first == board && it.second == r.id }) return true
            if (r.pointer?.kind == "issue" && issue == r.pointer.id) return true
            if (r.live?.kind == "native" && r.live.number != null && issue.trim() == "#${r.live.number}") return true
        }
        return r.attachments.any { a ->
            a.role == "work" && !a.released && a.pointer.kind == "block" && a.pointer.machine == machine && a.pointer.pane?.trim('%') == p.id.toString()
        }
    }
}

// ---- home

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(api: Api, model: HomeModel, fleet: HomeFleet, live: LiveChat) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    val coroutines = rememberCoroutineScope()
    val snack = remember { SnackbarHostState() }
    var making by remember { mutableStateOf(false) }
    var peek by remember { mutableStateOf<Peeked?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    /** What was answered here, by need and ask: folded away. */
    val folded = remember { mutableStateMapOf<String, Boolean>() }
    val project = scope.current

    suspend fun refresh(soon: Boolean = false) {
        val p = scope.current ?: return
        if (soon) model.soon(p.id) else model.load(p.id)
    }
    val toast: (String) -> Unit = { text -> coroutines.launch { snack.showSnackbar(text) } }

    LaunchedEffect(api) { fleet.load() }
    // Control's word, again every half minute; the stream says sooner.
    LaunchedEffect(project?.id) {
        while (isActive) {
            refresh()
            delay(30_000)
        }
    }
    // A message anywhere moves the people's counts; a board's thread changing is the board changing.
    LaunchedEffect(Unit) {
        snapshotFlow { live.rooms }.collectLatest { if (it > 0) refresh(soon = true) }
    }
    LaunchedEffect(Unit) {
        var was = live.changed.toMap()
        snapshotFlow { live.changed.toMap() }.collectLatest { now ->
            if (now.keys.any { now[it] != was[it] && model.boardOfThread(it) != null }) refresh(soon = true)
            was = now
        }
    }

    val state = HomeState(project, fleet, model)
    val needs = state.needs.filter { folded["${it.id}|${it.ask}"] != true }
    val people = model.rooms.filter { it.isConversation && !it.archived && scope.showsRoom(it) }
        .sortedWith(compareByDescending<ChatRoom> { if (it.unread > 0) 1 else 0 }.thenByDescending { it.last })
    val ideas = model.ideas.sortedByDescending { it.number }.map { i -> IdeaLine(i, model.hosted.firstOrNull { it.issueId == i.id }) }

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TabHeading("Arugula", project = true) {
                AnswerButton("New", Modifier.testTag("home-new"), primary = true) { making = true }
            }
            model.problem?.let { Text(it, Modifier.padding(horizontal = 16.dp), style = Face.mono(11.sp), color = Palette.danger, maxLines = 2) }
            PullToRefreshBox(
                refreshing,
                {
                    coroutines.launch {
                        refreshing = true
                        fleet.load()
                        refresh()
                        refreshing = false
                    }
                },
                Modifier.fillMaxSize(),
            ) {
                LazyColumn(Modifier.fillMaxSize().testTag("home-list")) {
                    item { SectionHead(if (needs.isEmpty()) NEEDS_YOU else "$NEEDS_YOU · ${needs.size}") }
                    if (needs.isEmpty()) {
                        item {
                            Text(if (model.loaded) "Nothing needs you." else "Looking…", Modifier.padding(horizontal = 16.dp, vertical = 6.dp).testTag("needs-none"), style = Face.ui(14.sp), color = Palette.dim)
                        }
                    }
                    items(needs, key = { "${it.id}|${it.ask}" }) { n ->
                        Column(Modifier.padding(horizontal = 16.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            NeedRow(n, Modifier.testTag("need-${n.id}").clickable {
                                when (val t = n.target) {
                                    is Need.Target.Pane -> peek = Peeked.Pane(t.m, t.p)
                                    is Need.Target.OnBoard -> nav.push(Dest.BoardThread(t.board))
                                    is Need.Target.Session -> peek = Peeked.Hosted(t.s)
                                }
                            })
                            NeedAnswer(n, api, model) { said ->
                                folded["${n.id}|${n.ask}"] = true
                                toast("${n.who}: $said")
                                coroutines.launch { refresh(soon = true) }
                            }
                        }
                        HorizontalDivider(color = Palette.raised)
                    }
                    if (ideas.isNotEmpty()) {
                        item { SectionHead("New ideas") }
                        items(ideas, key = { "idea-${it.issue.id}" }) { line ->
                            IdeaRow(line, Modifier.testTag("home-idea-${line.issue.number}").clickable {
                                val s = line.session
                                if (s != null) nav.push(Dest.Room(ChatRoom.thread(s.room, line.issue.title), thread = s.parent, title = line.issue.title))
                                else nav.push(Dest.ObjectThread(line.issue.id, line.issue.title))
                            })
                            HorizontalDivider(color = Palette.raised)
                        }
                    }
                    item { SectionHead("Boards") }
                    if (model.loaded && state.boards.isEmpty()) {
                        item { Text("No open boards", Modifier.padding(horizontal = 16.dp, vertical = 6.dp), style = Face.ui(14.sp), color = Palette.dim) }
                    }
                    items(state.boards, key = { "board-${it.board.id}" }) { b ->
                        BoardLineRow(b, Modifier.testTag("home-board-${b.board.title}").clickable { nav.push(Dest.BoardThread(b.board)) })
                        HorizontalDivider(color = Palette.raised)
                    }
                    item { SectionHead("People") }
                    items(people, key = { "room-${it.id}" }) { r ->
                        RoomRow(r) { nav.push(Dest.Room(r)) }
                    }
                    item { Spacer(Modifier.height(24.dp)) }
                }
            }
        }
        SnackbarHost(snack, Modifier.align(Alignment.BottomCenter))
    }

    if (making) {
        NewSheet(api, model, fleet, onDismiss = { making = false }, toast = toast, made = { coroutines.launch { refresh() } })
    }
    peek?.let { p -> SessionPeek(api, p, onDismiss = { peek = null }, toast = toast) }
}

/** A row of Needs you: a dot, who, how it stands, where, and what it asks. */
@Composable
private fun NeedRow(n: Need, modifier: Modifier) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            Dot(n.standing.tone)
            Text(n.who, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(n.standing.word, style = Face.mono(11.sp, FontWeight.Bold), color = n.standing.tone.color)
        }
        Text(n.on, style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (n.text.isNotEmpty()) Text(n.text, style = if (n.mono) Face.mono(12.sp) else Face.ui(14.sp), color = Palette.subtle, maxLines = 3, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
fun Dot(tone: Tone, size: Int = 7) = Box(Modifier.size(size.dp).clip(CircleShape).background(tone.color))

/** A board on home: its title and count, its lane bar, what's going on and who leads it. */
@Composable
private fun BoardLineRow(line: BoardLine, modifier: Modifier) {
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(line.board.title, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(if (line.total > 0) "${line.done}/${line.total}" else "–", Modifier.testTag("board-count"), style = Face.mono(12.sp, FontWeight.Bold), color = Palette.dim)
        }
        LaneBar(line.done, line.review, line.doing, line.total.coerceAtLeast(1))
        Text(
            buildAnnotatedString {
                line.says.forEachIndexed { i, (st, words) ->
                    if (i > 0) withStyle(SpanStyle(color = Palette.dim)) { append(" · ") }
                    withStyle(SpanStyle(color = if (st == Standing.IDLE) Palette.dim else st.tone.color)) { append(words) }
                }
            },
            style = Face.ui(13.sp), maxLines = 2,
        )
        Text(leadWords(line.board.lead), style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Who leads it: a session on a machine, or nobody. */
fun leadWords(l: Lead?): String {
    l ?: return "No lead"
    val on = l.machineName?.takeIf { it.isNotEmpty() }?.let { " on $it" } ?: ""
    return "Led by ${l.session}$on"
}

// ---- a board's thread

/**
 * A board's thread: its lane bar and who leads it over the conversation,
 * which reaches the session that leads it, and the sessions on its cards,
 * each opening its peek. Board opens its lanes.
 */
@Composable
fun BoardThreadScreen(api: Api, board: Board, model: HomeModel, fleet: HomeFleet, live: LiveChat) {
    val nav = LocalNav.current
    var peek by remember { mutableStateOf<Peeked?>(null) }
    val snack = remember { SnackbarHostState() }
    val coroutines = rememberCoroutineScope()
    val sessions = HomeState(LocalScope.current.current, fleet, model).sessions[board.id] ?: emptyList()
    val boardButton: @Composable () -> Unit = {
        Text("BOARD", Modifier.clickable { nav.push(Dest.Lanes(board)) }.padding(6.dp).testTag("open-board"), style = Face.mono(12.sp, FontWeight.Bold), color = Palette.accent)
    }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            DetailBar(board.title, boardButton)
            Loaded(key = board.id, load = { model.thread(board) }) { room, _ ->
                ChatRoomScreen(api, room, live, title = board.title, bar = false, heading = { BoardThreadHeading(board, model, sessions) { peek = it } })
            }
        }
        SnackbarHost(snack, Modifier.align(Alignment.BottomCenter))
    }
    peek?.let { p -> SessionPeek(api, p, onDismiss = { peek = null }, toast = { t -> coroutines.launch { snack.showSnackbar(t) } }) }
}

@Composable
private fun BoardThreadHeading(board: Board, model: HomeModel, sessions: List<BoardSession>, peek: (Peeked) -> Unit) {
    val line = model.boards.firstOrNull { it.id == board.id } ?: board
    val p = line.progress
    val total = p?.total ?: 0
    val done = p?.done ?: 0
    val review = p?.review ?: 0
    val doing = p?.doing ?: 0
    Column(Modifier.fillMaxWidth().background(Palette.void).padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
        LaneBar(done, review, doing, total.coerceAtLeast(1))
        Text("$done done · $review review · $doing doing · ${(total - done - review - doing).coerceAtLeast(0)} next", style = Face.mono(11.sp), color = Palette.dim)
        Text(leadWords(line.lead), Modifier.testTag("board-lead"), style = Face.mono(11.sp), color = Palette.subtle)
        if (sessions.isNotEmpty()) {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (s in sessions) {
                    ToneCard(s.standing.tone, Modifier.widthIn(max = 240.dp).clickable { peek(s.peeked) }) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            Dot(s.standing.tone, 6)
                            Text(s.name, style = Face.ui(13.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        Text("${s.card} · ${s.word}", style = Face.mono(10.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}
