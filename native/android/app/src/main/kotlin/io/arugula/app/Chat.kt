// Chat: the current project's team's channels and DMs (every room in the
// personal project), and one room, read the way a team chat reads: an avatar
// beside each run of messages, the name and time on one line, the text tight
// under it, and a long message folded. Chat's live stream (`LiveChat`) says
// when to look again, as the web's and the iPhone's do.

package io.arugula.app

import android.os.Handler
import android.os.Looper
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.arugula.core.ChatLive
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import kotlin.math.min
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Chat's live stream (`GET /api/chat/live`, the core's ChatLive): what to
 * fetch again as it happens. [rooms] moves when the room list's counts or
 * rooms do, [changed] for a room when its messages do, and [epoch] when a new
 * stream starts (its `hello`: anything could have been missed). A dropped
 * stream is opened again; the room on screen is told to control, so nobody
 * is pushed for what they're reading.
 */
class LiveChat(private val api: Api) {
    var rooms by mutableIntStateOf(0)
        private set
    val changed = mutableStateMapOf<String, Int>()
    var epoch by mutableIntStateOf(0)
        private set
    var connected by mutableStateOf(false)
        private set
    private var live: ChatLive? = null
    private var opening = false
    private var failures = 0
    private var stream: String? = null
    private var shown: String? = null
    private val main = Handler(Looper.getMainLooper())
    private val scope = MainScope()

    /** Open the stream, unless it's open or opening. */
    fun start() {
        if (live != null || opening) return
        opening = true
        scope.launch {
            val got = withContext(Dispatchers.IO) {
                runCatching {
                    var c: ChatLive? = null
                    // The core's thread says so; the main thread takes it, and
                    // a late call after close (or before it's kept) finds nothing.
                    c = ChatLive(api.control) { main.post { if (c != null && live === c) pump() } }
                    c
                }.getOrNull()
            }
            opening = false
            if (got != null) {
                live = got
                pump()
            } else {
                retry()
            }
        }
    }

    /** The room on screen, or none. */
    fun open(room: String?) {
        shown = room
        tell()
    }

    private fun tell() {
        val s = stream ?: return
        val body = JSONObject().put("stream", s).put("room", shown ?: JSONObject.NULL)
            .put("people", org.json.JSONArray()).put("machines_online", org.json.JSONArray())
        scope.launch { runCatching { api.post("/api/chat/live/open", body) } }
    }

    private fun pump() {
        val l = live ?: return
        while (true) {
            val s = l.next() ?: break
            val ev = runCatching { JSONObject(s) }.getOrNull() ?: continue
            val room = ev.str("room")
            when (val type = ev.optString("type")) {
                "hello" -> {
                    stream = ev.str("stream")
                    connected = true
                    failures = 0
                    epoch++
                    rooms++
                    tell()
                }
                "message", "changed", "reacted", "pinned", "reply" -> {
                    if (room != null) changed[room] = (changed[room] ?: 0) + 1
                    if (type == "message") rooms++
                }
                "room", "read" -> rooms++
                "closed" -> {
                    live = null
                    l.close()
                    stream = null
                    connected = false
                    retry()
                    return
                }
            }
        }
    }

    /** Open it again: in 3 seconds, then longer each time it fails, up to a minute. */
    private fun retry() {
        failures++
        val wait = min(60, 3 * (1 shl min(failures - 1, 5)))
        scope.launch {
            delay(wait * 1000L)
            start()
        }
    }
}

@Composable
fun ChatRoomsScreen(api: Api, live: LiveChat) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    Column(Modifier.fillMaxSize()) {
        TabHeading("Chat", project = true)
        // The stream says when the list's counts move; a slow look again covers a stream that's down.
        Loaded(key = scope.current?.id, every = 60_000, trigger = live.rooms, load = { api.get("/api/chat").list("rooms", ChatRoom::of) }) { all, _ ->
            val rooms = all.filter { it.isConversation && !it.archived && scope.showsRoom(it) }
                .sortedWith(compareByDescending<ChatRoom> { if (it.unread > 0) 1 else 0 }.thenByDescending { it.last })
            val channels = rooms.filter { it.kind == "channel" }
            val dms = rooms.filter { it.kind == "dm" }
            if (rooms.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps("No rooms yet") }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    if (channels.isNotEmpty()) {
                        item { SectionHead("Channels") }
                        items(channels, key = { it.id }) { r -> RoomRow(r) { nav.push(Dest.Room(r)) } }
                    }
                    if (dms.isNotEmpty()) {
                        item { SectionHead("Direct messages") }
                        items(dms, key = { it.id }) { r -> RoomRow(r) { nav.push(Dest.Room(r)) } }
                    }
                }
            }
        }
    }
}

@Composable
fun SectionHead(text: String) = Caps(text, Modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp))

/** A room in a list: a channel's # or a DM's avatar, its name, and how many messages wait in it. */
@Composable
fun RoomRow(room: ChatRoom, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 11.dp).testTag("room-${room.name}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (room.kind == "channel") {
            Text("#", Modifier.width(20.dp), style = Face.mono(15.sp, FontWeight.Bold), color = Palette.dim)
        } else {
            Avatar(room.title, size = 20)
        }
        val bold = room.unread > 0
        Text(
            if (room.kind == "channel") room.name else room.title, Modifier.weight(1f),
            style = Face.ui(15.sp, if (bold) FontWeight.Bold else FontWeight.Normal), color = if (bold) Palette.fg else Palette.subtle,
            maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        if (room.unread > 0) {
            Text(
                "${room.unread}",
                Modifier.clip(CircleShape).background(if (room.mentions > 0) Palette.danger else Palette.accent).padding(horizontal = 6.dp, vertical = 1.dp),
                style = Face.mono(11.sp, FontWeight.Bold), color = Palette.void,
            )
        }
    }
}

/** The stream's state, for a room's bar: live, or coming back. */
@Composable
fun LiveDot(live: LiveChat) {
    val c = if (live.connected) Palette.ok else Palette.warn
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Box(Modifier.size(6.dp).clip(CircleShape).background(c))
        Text(if (live.connected) "LIVE" else "RECONNECTING", Modifier.testTag("chat-live"), style = Face.mono(10.sp, FontWeight.Bold).copy(letterSpacing = 0.8.sp), color = c)
    }
}

/**
 * A room: its messages and a composer. [thread] reads the replies to that
 * message instead of the room's list (a session's conversation hangs off its
 * first message). [heading] goes over the messages (a board's lane bar).
 */
@Composable
fun ChatRoomScreen(
    api: Api, room: ChatRoom, live: LiveChat, title: String? = null, thread: Long? = null,
    bar: Boolean = true, trailing: @Composable () -> Unit = {}, heading: (@Composable () -> Unit)? = null,
) {
    var messages by remember(room.id, thread) { mutableStateOf<List<ChatMessage>>(emptyList()) }
    var draft by remember(room.id, thread) { mutableStateOf("") }
    var problem by remember { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    val coroutines = rememberCoroutineScope()
    val list = rememberLazyListState()

    suspend fun load() {
        try {
            val path = "/api/chat/rooms/${room.id}/messages" + (thread?.let { "?thread=$it" } ?: "")
            messages = api.get(path).list("messages", ChatMessage::of).sortedBy { it.seq }
            problem = null
            messages.lastOrNull()?.seq?.let { last ->
                val read = thread?.let { "/api/chat/rooms/${room.id}/threads/$it/read" } ?: "/api/chat/rooms/${room.id}/read"
                runCatching { api.post(read, JSONObject().put("seq", last)) }
            }
        } catch (e: Exception) {
            if (e is kotlinx.coroutines.CancellationException) throw e
            problem = describe(e)
        }
    }

    // The stream says when this room changed; a slow look again covers a stream that's down.
    LaunchedEffect(room.id, thread) {
        while (isActive) {
            load()
            delay(if (live.connected) 60_000 else 5_000)
        }
    }
    LaunchedEffect(live.changed[room.id], live.epoch) { load() }
    DisposableEffect(room.id) {
        live.open(room.id)
        onDispose { live.open(null) }
    }
    val shown = messages.filter { !it.deleted }
    LaunchedEffect(shown.lastOrNull()?.seq) { if (shown.isNotEmpty()) list.animateScrollToItem(shown.lastIndex) }

    Column(Modifier.fillMaxSize().background(Palette.bg)) {
        if (bar) DetailBar(title ?: room.title) { trailing(); LiveDot(live) }
        heading?.invoke()
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list) {
            items(shown.size, key = { shown[it].seq }) { i ->
                val m = shown[i]
                val prev = shown.getOrNull(i - 1)
                Column {
                    if (prev == null || !sameDay(prev.at, m.at)) DayRule(m.at)
                    MessageView(m, prev != null && continues(prev, m))
                }
            }
            item { Spacer(Modifier.height(8.dp)) }
        }
        problem?.let { Text(it, Modifier.padding(horizontal = 16.dp), style = Face.mono(12.sp), color = Palette.danger) }
        if (room.writable) {
            val empty = draft.isBlank()
            Row(Modifier.fillMaxWidth().background(Palette.bg).padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    draft, { draft = it }, Modifier.weight(1f).testTag("chat-draft"),
                    placeholder = { Text(if (thread == null) "Message ${title ?: room.title}" else "Reply", style = Face.ui(16.sp), color = Palette.dim, maxLines = 1) },
                    textStyle = Face.ui(16.sp).copy(color = Palette.fg), maxLines = 6, shape = RoundedCornerShape(10.dp),
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Palette.surface, focusedContainerColor = Palette.surface, unfocusedBorderColor = Palette.raised),
                )
                IconButton(
                    {
                        coroutines.launch {
                            sending = true
                            try {
                                val body = JSONObject().put("text", draft)
                                thread?.let { body.put("parent", it) }
                                api.post("/api/chat/rooms/${room.id}/messages", body)
                                draft = ""
                                load()
                            } catch (e: Exception) {
                                problem = describe(e)
                            } finally {
                                sending = false
                            }
                        }
                    },
                    Modifier.size(48.dp).testTag("chat-send"),
                    enabled = !empty && !sending,
                    shape = RoundedCornerShape(10.dp),
                    colors = IconButtonDefaults.iconButtonColors(containerColor = Palette.accent, contentColor = Palette.void, disabledContainerColor = Palette.muted, disabledContentColor = Palette.void),
                ) { Icon(Icons.AutoMirrored.Filled.Send, "Send") }
            }
        }
    }
}

/** A thread found by its object, as a New idea's issue has one. */
@Composable
fun ObjectThreadScreen(api: Api, target: String, title: String, live: LiveChat) {
    Loaded(key = target, load = { api.threadOf(target) }) { room, _ -> ChatRoomScreen(api, room, live, title = title) }
}

/** The same person (or agent) again within five minutes, on the same day: no avatar, name and time over it. */
private fun continues(a: ChatMessage, b: ChatMessage) =
    a.from == b.from && a.author == b.author && b.at - a.at < 5 * 60 * 1000 && sameDay(a.at, b.at)

private fun sameDay(a: Long, b: Long): Boolean {
    val x = Calendar.getInstance().apply { timeInMillis = a }
    val y = Calendar.getInstance().apply { timeInMillis = b }
    return x.get(Calendar.YEAR) == y.get(Calendar.YEAR) && x.get(Calendar.DAY_OF_YEAR) == y.get(Calendar.DAY_OF_YEAR)
}

/** A day's line between messages. */
@Composable
private fun DayRule(at: Long) {
    val words = if (sameDay(at, System.currentTimeMillis())) "Today" else SimpleDateFormat("EEEE, MMMM d", Locale.getDefault()).format(Date(at))
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        HorizontalDivider(Modifier.weight(1f), color = Palette.raised)
        Caps(words)
        HorizontalDivider(Modifier.weight(1f), color = Palette.raised)
    }
}

private const val FOLD_LINES = 12
private const val FOLD_CHARS = 900

/** One message: the avatar beside the first of a run with the name and time on one line, then the text, tight. */
@Composable
private fun MessageView(m: ChatMessage, continues: Boolean) {
    var expanded by remember { mutableStateOf(false) }
    val long = m.text.length > FOLD_CHARS || m.text.split('\n').size > FOLD_LINES
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = if (continues) 2.dp else 10.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (continues) Spacer(Modifier.width(36.dp)) else Box(Modifier.padding(top = 2.dp)) { Avatar(m.name.ifEmpty { m.from }, agent = m.agentMachine != null) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            if (!continues) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(m.name.ifEmpty { m.from }, style = Face.ui(15.sp, FontWeight.Bold), color = Palette.fg, maxLines = 1)
                    if (m.agentMachine != null) {
                        Text("AGENT", Modifier.border(1.dp, Palette.teal.copy(alpha = 0.6f), RoundedCornerShape(3.dp)).padding(horizontal = 4.dp, vertical = 1.dp), style = Face.mono(9.sp, FontWeight.Bold), color = Palette.teal)
                        Text("on ${m.agentMachine}", style = Face.mono(11.sp), color = Palette.dim, maxLines = 1)
                    } else if (!m.via.isNullOrEmpty()) {
                        Text("via ${m.via}", style = Face.mono(11.sp), color = Palette.dim)
                    }
                    Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(m.at)), style = Face.mono(11.sp), color = Palette.dim)
                }
            }
            m.quote?.takeIf { it.isNotEmpty() }?.let { CodeBlock(it) }
            if (m.text.isNotEmpty()) {
                ChatText(if (long && !expanded) fold(m.text) else m.text)
                if (long) Text(if (expanded) "Show less" else "Show more", Modifier.clickable { expanded = !expanded }, style = Face.mono(12.sp, FontWeight.Bold), color = Palette.accent)
            }
            if (m.edited) Text("(edited)", style = Face.mono(10.sp), color = Palette.muted)
            for (f in m.files) Text("📎 $f", style = Face.mono(12.sp), color = Palette.subtle)
            if (m.reactions.isNotEmpty()) {
                Row(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (r in m.reactions) {
                        Text(
                            "${r.emoji} ${r.count}",
                            Modifier.clip(CircleShape).background(if (r.mine) Palette.accent.copy(alpha = 0.18f) else Palette.raised.copy(alpha = 0.6f)).padding(horizontal = 7.dp, vertical = 2.dp),
                            style = Face.mono(12.sp), color = Palette.fg,
                        )
                    }
                }
            }
        }
    }
}

/** The first lines of a long message, with any code block it cuts into closed. */
private fun fold(text: String): String {
    var out = text.split('\n').take(FOLD_LINES).joinToString("\n")
    if (out.length > FOLD_CHARS) out = out.take(FOLD_CHARS)
    val fences = out.split('\n').count { it.trim().startsWith("```") }
    return out + (if (fences % 2 == 1) "\n```" else "") + " …"
}

/**
 * A message's text with the chat's small Markdown, as the web shows it:
 * fenced code as a block, `code`, **bold**, *italic*, links; headings and
 * bullets as bold lines and dots. Nothing in a code block is formatted.
 */
@Composable
fun ChatText(text: String) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (part in parts(text)) {
            if (part.code) CodeBlock(part.text) else SelectionContainer { Text(inline(part.text), style = Face.ui(15.sp).copy(lineHeight = 21.sp), color = Palette.fg) }
        }
    }
}

private data class Part(val code: Boolean, val text: String)

private fun parts(text: String): List<Part> {
    val out = mutableListOf<Part>()
    val prose = mutableListOf<String>()
    var code: MutableList<String>? = null
    fun flush() {
        val s = prose.joinToString("\n").trim('\n')
        if (s.isNotEmpty()) out += Part(false, s)
        prose.clear()
    }
    for (line in text.split('\n')) {
        when {
            line.trim().startsWith("```") -> if (code != null) {
                out += Part(true, code.joinToString("\n"))
                code = null
            } else {
                flush()
                code = mutableListOf()
            }
            code != null -> code.add(line)
            else -> prose += line
        }
    }
    code?.let { out += Part(true, it.joinToString("\n")) }
    flush()
    return out
}

private val inlineRe = Regex("""`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s][^*]*)\*|\[([^\]]+)\]\(([^)\s]+)\)|(https?://[^\s)]+)""")

/** Inline Markdown, keeping the lines; a heading becomes a bold line and a bullet a dot. */
private fun inline(s: String): AnnotatedString = buildAnnotatedString {
    val lines = s.split('\n')
    lines.forEachIndexed { n, raw ->
        val t = raw.trimStart(' ')
        var line = raw
        var bold = false
        if (t.startsWith("#")) {
            val title = t.trimStart('#').trim()
            if (title.isNotEmpty()) {
                line = title
                bold = true
            }
        } else {
            for (b in listOf("- ", "* ", "+ ")) if (t.startsWith(b)) {
                line = " ".repeat(raw.length - t.length) + "• " + t.drop(2)
                break
            }
        }
        if (bold) pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
        var at = 0
        for (m in inlineRe.findAll(line)) {
            append(line.substring(at, m.range.first))
            val g = m.groupValues
            when {
                g[1].isNotEmpty() -> withStyle(SpanStyle(fontFamily = Face.mono, background = Palette.raised, color = Palette.peach, fontSize = 13.sp)) { append(g[1]) }
                g[2].isNotEmpty() -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(g[2]) }
                g[3].isNotEmpty() -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append(g[3]) }
                g[4].isNotEmpty() -> withLink(LinkAnnotation.Url(g[5], TextLinkStyles(SpanStyle(color = Palette.accent)))) { append(g[4]) }
                else -> withLink(LinkAnnotation.Url(g[6], TextLinkStyles(SpanStyle(color = Palette.accent)))) { append(g[6]) }
            }
            at = m.range.last + 1
        }
        append(line.substring(at))
        if (bold) pop()
        if (n < lines.lastIndex) append('\n')
    }
}

/** Lines shown as they are: monospaced, scrolling sideways when long. */
@Composable
fun CodeBlock(text: String) {
    Box(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(Palette.void).border(1.dp, Palette.raised, RoundedCornerShape(8.dp))
            .horizontalScroll(rememberScrollState()).padding(10.dp),
    ) {
        SelectionContainer { Text(text, style = Face.mono(13.sp), color = Palette.fg, softWrap = false) }
    }
}
