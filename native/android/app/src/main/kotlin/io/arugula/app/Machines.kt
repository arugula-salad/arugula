// Machines: the account's, reached through control's relay, their panes, and
// a pane's terminal drawn from the core's VT state (`Term`), as the iPhone's
// MachineViews.swift and TerminalView.swift.

package io.arugula.app

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key as HwKey
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.arugula.core.Frame
import io.arugula.core.Key
import io.arugula.core.Mods
import io.arugula.core.Row as TermRow
import io.arugula.core.Term
import kotlinx.coroutines.launch
import org.json.JSONObject

@Composable
fun MachinesScreen(api: Api, fleet: HomeFleet) {
    val nav = LocalNav.current
    val prefs = LocalContext.current.getSharedPreferences("arugula", Context.MODE_PRIVATE)
    Column(Modifier.fillMaxSize()) {
        TabHeading("Machines")
        Loaded(key = "machines", load = { ordered(api.get("/api/directory").list("daemons", Machine::of), prefs.getString(LAST_MACHINE, null)) }) { machines, _ ->
            if (machines.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps("No machines yet") }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(machines, key = { it.id }) { m ->
                        Row(
                            Modifier.fillMaxWidth().clickable {
                                prefs.edit().putString(LAST_MACHINE, m.id).apply()
                                nav.push(Dest.MachineD(m))
                            }.padding(horizontal = 16.dp, vertical = 12.dp).testTag("machine-${m.name}"),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Dot(if (m.online) Tone.OK else Tone.MUTED, 8)
                            Column {
                                Text(m.name, style = Face.ui(16.sp, FontWeight.SemiBold), color = Palette.fg)
                                Caps(if (m.online) "online" else "offline")
                            }
                        }
                        HorizontalDivider(color = Palette.raised)
                    }
                }
            }
        }
    }
}

/** The machine picked last, first of yours. */
private const val LAST_MACHINE = "last-machine"

/**
 * In the phone's order, the core's (Arugula #1355): your online machines,
 * the one you used last first, then teammates', by name, and the offline
 * ones after.
 */
private fun ordered(machines: List<Machine>, recent: String?): List<Machine> {
    val order = sections(machines.map { it.host() }, null, recent)?.optJSONArray("order").objects()
    if (order.isEmpty()) return machines
    return order.mapNotNull { o -> machines.firstOrNull { it.id == o.optString("id") } }
}

/** The machine's link: home's, when it keeps one, else one for this screen. */
@Composable
private fun sessionFor(api: Api, machine: Machine, fleet: HomeFleet): MachineSession {
    val kept = fleet.find(machine.id)
    val own = remember(machine.id) { if (kept == null) MachineSession(api, machine) else null }
    DisposableEffect(own) { onDispose { own?.disconnect() } }
    return kept ?: own!!
}

/** A machine's panes in the current project as cards, the ones that want you first; one opens its terminal. */
@Composable
fun MachineScreen(api: Api, machine: Machine, fleet: HomeFleet) {
    val nav = LocalNav.current
    val shown = LocalScope.current.current?.shown
    val session = sessionFor(api, machine, fleet)
    val coroutines = rememberCoroutineScope()
    var problem by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(session) { session.connect() }
    Column(Modifier.fillMaxSize()) {
        DetailBar(machine.name) {
            AnswerButton("New terminal", Modifier.testTag("new-terminal"), primary = true, enabled = session.connected) {
                coroutines.launch {
                    try {
                        val body = session.request("POST", "/api/run", JSONObject()) as? JSONObject
                        val pane = body?.int("pane") ?: throw IllegalStateException("the machine sent no pane")
                        nav.push(Dest.Terminal(machine.id, pane, body.int("tab") ?: 0))
                    } catch (e: Exception) {
                        problem = describe(e)
                    }
                }
            }
        }
        (session.problem ?: problem)?.let { Text(it, Modifier.padding(16.dp).testTag("machine-problem"), style = Face.mono(12.sp), color = Palette.danger) }
        if (!session.connected && session.problem == null) {
            Loading()
            return@Column
        }
        // Only the project's sessions' panes, as the core's rule keeps them (Arugula #1352).
        val cards = session.cards(shown)
        val kept = cards.flatMap { it.panes }.toSet()
        val needs = cards.sumOf { it.needsYou }
        val panes = session.panes.filter { shown == null || it.id in kept }.sortedWith(compareBy({ it.order }, { -(it.lastMs ?: 0) }))
        LazyColumn(Modifier.fillMaxSize(), contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item { Caps(if (needs > 0) "${panes.size} panes · $needs need you" else "${panes.size} panes", Modifier.testTag("machine-count")) }
            items(panes, key = { it.id }) { p ->
                val tone = when (p.order) {
                    0 -> Tone.ACCENT
                    1 -> Tone.INFO
                    2 -> Tone.OK
                    else -> Tone.MUTED
                }
                ToneCard(tone, Modifier.clickable { nav.push(Dest.Terminal(machine.id, p.id, p.tab)) }.testTag("pane-${p.id}"), glow = p.order == 0) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text((p.attention ?: "idle").replace('_', ' ').uppercase(), style = Face.mono(10.sp, FontWeight.Bold), color = tone.color)
                        p.program?.let { Caps(it) }
                    }
                    Text(session.conversations[p.id] ?: p.name, style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    p.headline?.takeIf { it.isNotEmpty() }?.let { Text(it, style = Face.ui(13.sp), color = Palette.subtle, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                    p.cwd?.let { Text(it, style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                }
            }
        }
    }
}

private fun rgb(c: Int) = Color(0xFF000000.toInt() or (c and 0xFFFFFF))

/**
 * A pane's terminal: the core's `Term` takes the pane's bytes, sized to the
 * screen, and its changed rows are kept and drawn on a Canvas in the mono
 * face. Typing goes through a hidden text field (the soft keyboard), hardware
 * keys through their codes, and the key bar has what a phone's keyboard
 * hasn't: Esc, Tab, Ctrl, the arrows.
 */
@Composable
fun TerminalScreen(api: Api, d: Dest.Terminal, fleet: HomeFleet) {
    val machine = fleet.find(d.machine)?.machine ?: remember(d.machine) { Machine(d.machine, "Machine", true) }
    val session = sessionFor(api, machine, fleet)
    val term = remember(d.machine, d.pane) { Term(80, 24) }
    val rows = remember(term) { mutableMapOf<Int, TermRow>() }
    var frame by remember(term) { mutableStateOf<Frame?>(null) }
    var drawn by remember(term) { mutableIntStateOf(0) }
    var attached by remember(term) { mutableStateOf(false) }
    var grid by remember(term) { mutableStateOf(0 to 0) }
    var ctrl by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val context = LocalContext.current
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val style = Face.mono(13.sp)
    val cell = remember(measurer) { measurer.measure("M", style).size }
    val cellW = cell.width.toFloat()
    val cellH = cell.height.toFloat()

    DisposableEffect(session, term) {
        session.terms[d.pane] = term
        onDispose {
            session.terms.remove(d.pane)
            session.detach(intArrayOf(d.pane))
            term.close()
        }
    }
    LaunchedEffect(session) { session.connect() }
    // A new channel after a reconnect: ask for the pane again, snapshot first.
    LaunchedEffect(session.connected) { if (!session.connected) attached = false }
    // Size: the grid that fits, told to the terminal and the machine.
    LaunchedEffect(grid, session.connected) {
        val (cols, rs) = grid
        if (cols == 0 || !session.connected) return@LaunchedEffect
        term.resize(cols, rs, cellW.toInt(), cellH.toInt())
        if (attached) session.view(d.tab, d.pane, cols, rs) else {
            attached = true
            session.attach(d.tab, d.pane, cols, rs)
        }
        session.pump()
    }
    // What the pane sent, drawn: only the rows that changed.
    LaunchedEffect(session.frames, grid) {
        val f = term.render()
        if (f.dirty == Frame.FULL) rows.clear()
        for (r in f.changed) rows[r.y] = r
        frame = f
        drawn++
    }

    fun send(bytes: ByteArray) {
        if (bytes.isEmpty()) return
        term.scrollToBottom()
        session.input(d.pane, bytes)
    }
    fun key(code: Int, mods: Int = 0) = send(term.key(code, mods))
    fun typed(text: String) {
        for (ch in text) {
            when {
                ch == '\n' -> key(Key.ENTER)
                ctrl && ch.uppercaseChar() in 'A'..'Z' -> {
                    ctrl = false
                    key(ch.uppercaseChar().code, Mods.CTRL)
                }
                else -> {
                    ctrl = false
                    send(ch.toString().toByteArray())
                }
            }
        }
    }

    val title = session.conversations[d.pane] ?: session.panes.firstOrNull { it.id == d.pane }?.name ?: "Terminal"
    Column(Modifier.fillMaxSize().background(Palette.void)) {
        DetailBar(title) { Caps(machine.name) }
        session.problem?.let { Text(it, Modifier.padding(horizontal = 12.dp), style = Face.mono(12.sp), color = Palette.danger) }
        // The soft keyboard types into this: one space, so a backspace shows as an empty field.
        var field by remember { mutableStateOf(TextFieldValue(" ", TextRange(1))) }
        BasicTextField(
            field,
            { v ->
                when {
                    v.text.isEmpty() -> key(Key.BACKSPACE)
                    v.text.length > 1 && v.text.startsWith(" ") -> typed(v.text.substring(1))
                    v.text != " " -> typed(v.text)
                }
                field = TextFieldValue(" ", TextRange(1))
            },
            Modifier.size(1.dp).alpha(0f).focusRequester(focus).testTag("term-input").onPreviewKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                val mods = (if (e.isShiftPressed) Mods.SHIFT else 0) or (if (e.isCtrlPressed) Mods.CTRL else 0) or (if (e.isAltPressed) Mods.ALT else 0)
                val code = when (e.key) {
                    HwKey.Escape -> Key.ESCAPE
                    HwKey.Tab -> Key.TAB
                    HwKey.Enter, HwKey.NumPadEnter -> Key.ENTER
                    HwKey.Backspace -> Key.BACKSPACE
                    HwKey.DirectionUp -> Key.UP
                    HwKey.DirectionDown -> Key.DOWN
                    HwKey.DirectionLeft -> Key.LEFT
                    HwKey.DirectionRight -> Key.RIGHT
                    HwKey.MoveHome -> Key.HOME
                    HwKey.MoveEnd -> Key.END
                    HwKey.PageUp -> Key.PAGE_UP
                    HwKey.PageDown -> Key.PAGE_DOWN
                    HwKey.Delete -> Key.DELETE
                    else -> null
                }
                val letter = e.utf16CodePoint.takeIf { e.isCtrlPressed }?.toChar()?.uppercaseChar()?.takeIf { it in 'A'..'Z' }
                when {
                    code != null -> key(code, mods).let { true }
                    letter != null -> key(letter.code, mods).let { true }
                    else -> false
                }
            },
        )
        // What it shows, as words: for a screen reader, and the UI test.
        val text = remember(drawn) { (0 until (frame?.rows ?: 0)).joinToString("\n") { rows[it]?.text?.trimEnd() ?: "" } }
        BoxWithConstraints(
            Modifier.weight(1f).fillMaxWidth().padding(6.dp).clickable(indication = null, interactionSource = null) {
                focus.requestFocus()
                keyboard?.show()
            }.semantics { contentDescription = text }.testTag("terminal"),
        ) {
            val cols = with(density) { (maxWidth.toPx() / cellW).toInt() }.coerceAtLeast(10)
            val rs = with(density) { (maxHeight.toPx() / cellH).toInt() }.coerceAtLeast(4)
            LaunchedEffect(cols, rs) { grid = cols to rs }
            Canvas(Modifier.fillMaxSize()) {
                // Read here, so each new frame draws again.
                drawn.let { }
                val f = frame ?: return@Canvas
                if (f.bg != Frame.DEFAULT_BG) drawRect(rgb(f.bg))
                for ((y, row) in rows) {
                    if (y >= f.rows) continue
                    for (run in row.runs) {
                        val x = run.col * cellW
                        if (run.bg != Frame.DEFAULT_BG) drawRect(rgb(run.bg), Offset(x, y * cellH), Size(run.cells * cellW, cellH))
                        if (run.text.isBlank()) continue
                        val s = style.copy(
                            color = rgb(run.fg),
                            fontWeight = if (run.flags and io.arugula.core.Run.BOLD != 0) FontWeight.Bold else FontWeight.Normal,
                            fontStyle = if (run.flags and io.arugula.core.Run.ITALIC != 0) FontStyle.Italic else FontStyle.Normal,
                        )
                        if (run.cells == run.text.length) {
                            drawText(measurer, run.text, Offset(x, y * cellH), s, softWrap = false)
                        } else {
                            // Wide or combined characters: one cell each, so they stay on the grid.
                            drawText(measurer, run.text, Offset(x, y * cellH), s, softWrap = false, size = Size(run.cells * cellW, cellH))
                        }
                    }
                }
                if (f.cursorVisible) drawRect(rgb(f.cursorColor).copy(alpha = 0.7f), Offset(f.cursorX * cellW, f.cursorY * cellH), Size(cellW, cellH))
            }
        }
        // What a phone's keyboard hasn't.
        Row(Modifier.fillMaxWidth().background(Palette.surface).horizontalScroll(rememberScrollState()).padding(6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            KeyButton("esc") { key(Key.ESCAPE) }
            KeyButton("tab") { key(Key.TAB) }
            KeyButton("ctrl", on = ctrl) { ctrl = !ctrl }
            KeyButton("↑") { key(Key.UP) }
            KeyButton("↓") { key(Key.DOWN) }
            KeyButton("←") { key(Key.LEFT) }
            KeyButton("→") { key(Key.RIGHT) }
            for (s in listOf("/", "-", "|", "~")) KeyButton(s) { send(s.toByteArray()) }
            KeyButton("paste") {
                val clip = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clip.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString()?.let { send(it.toByteArray()) }
            }
            KeyButton("⌨") {
                focus.requestFocus()
                keyboard?.show()
            }
        }
    }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
}

@Composable
private fun KeyButton(label: String, on: Boolean = false, onClick: () -> Unit) {
    Text(
        label,
        Modifier.clip(CircleShape).background(if (on) Palette.accent else Palette.raised).clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 6.dp).testTag("key-$label"),
        style = Face.mono(14.sp), color = if (on) Palette.void else Palette.fg,
    )
}
