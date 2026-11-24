// Machines (#1358): one page of machine › session card › one-line pane rows,
// the project's sessions only, in the core's order (Arugula #1355), as the
// web's Machines page. Machines with nothing in the project fold into one
// line, and an offline one can't be opened. On a tablet the page sits beside
// the session's live tab (ListDetailPaneScaffold), and before anything is
// picked that's where you were; on a phone a session opens a tab at a time,
// its panes stacked and live. Any pane opens full screen, with the keyboard
// and the key bar: a pane's terminal drawn from the core's VT state (`Term`),
// as the iPhone's MachineViews.swift and TerminalView.swift.

package io.arugula.app

import android.content.ClipboardManager
import android.content.Context
import android.content.SharedPreferences
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldDefaults
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem
import androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective
import androidx.compose.material3.adaptive.layout.calculateThreePaneScaffoldValue
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.withTransform
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
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
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

/** Machines is beside what it opened (a tablet), or a page at a time (a phone). */
private val LocalWide = staticCompositionLocalOf { false }

/** Open [d]: beside the page on a tablet, in its place, else over it. */
private fun Nav.show(wide: Boolean, d: Dest) = if (wide) replace(d) else push(d)

/**
 * Machines: the page, and what it opened beside it on a tablet. [top] is
 * the tab's open destination; [detail] draws it. A terminal isn't here: it
 * takes the whole screen.
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun MachinesScreen(api: Api, fleet: HomeFleet, home: HomeModel, top: Dest?, detail: @Composable (Dest) -> Unit) {
    val directive = calculatePaneScaffoldDirective(currentWindowAdaptiveInfo()).copy(horizontalPartitionSpacerSize = 0.dp)
    val wide = directive.maxHorizontalPartitions > 1
    // The tab's own stack says what's open; the scaffold only lays it out.
    val value = calculateThreePaneScaffoldValue(
        directive.maxHorizontalPartitions,
        ListDetailPaneScaffoldDefaults.adaptStrategies(),
        ThreePaneScaffoldDestinationItem<Nothing>(if (top == null) ListDetailPaneScaffoldRole.List else ListDetailPaneScaffoldRole.Detail),
    )
    CompositionLocalProvider(LocalWide provides wide) {
        ListDetailPaneScaffold(
            directive, value,
            listPane = {
                AnimatedPane(Modifier.preferredWidth(380.dp)) {
                    Row(Modifier.fillMaxSize()) {
                        Box(Modifier.weight(1f)) { MachinesPage(fleet, top) }
                        if (wide) VerticalDivider(color = Palette.raised)
                    }
                }
            },
            detailPane = {
                AnimatedPane {
                    when {
                        top != null -> detail(top)
                        wide -> WhereYouWere(api, fleet, home)
                    }
                }
            },
        )
    }
}

/** The machine picked last, first of yours. */
private const val LAST_MACHINE = "last-machine"

/** Where you were in a project, on this device: the pane you last had open there. */
private const val WAS = "last-pane."

data class KeptPane(val machine: String, val tab: Int, val pane: Int)

private fun SharedPreferences.keptPane(project: String): KeptPane? {
    val parts = getString(WAS + project, null)?.split(' ')?.takeIf { it.size == 3 } ?: return null
    return KeptPane(parts[0], parts[1].toIntOrNull() ?: return null, parts[2].toIntOrNull() ?: return null)
}

/** You were here: the pane, in the project, and its machine is the one used last. */
private fun SharedPreferences.keep(project: String?, k: KeptPane) {
    val e = edit().putString(LAST_MACHINE, k.machine)
    project?.let { e.putString(WAS + it, "${k.machine} ${k.tab} ${k.pane}") }
    e.apply()
}

@Composable
private fun prefs(): SharedPreferences = LocalContext.current.getSharedPreferences("arugula", Context.MODE_PRIVATE)

/**
 * The page: each machine with sessions in the project, in the core's order,
 * its session cards and their panes as one-line rows; then the rest folded
 * into one line. The core decides which show and in what order (`aru_machines`).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MachinesPage(fleet: HomeFleet, top: Dest?) {
    val project = LocalScope.current.current
    val prefs = prefs()
    val coroutines = rememberCoroutineScope()
    var refreshing by remember { mutableStateOf(false) }
    var unfolded by rememberSaveable(project?.id) { mutableStateOf(false) }
    // The directory again, each visit: a machine that came online shows.
    LaunchedEffect(Unit) { fleet.load() }
    val was = project?.let { prefs.keptPane(it.id) }
    // Read again as any machine's panes move.
    val page = Sections.of(sections(fleet.directory.map { m -> fleet.find(m.id)?.host() ?: m.host() }, project?.shown, prefs.getString(LAST_MACHINE, null)))
    val open = top as? Dest.Session

    Column(Modifier.fillMaxSize()) {
        TabHeading("Machines")
        PullToRefreshBox(
            refreshing,
            {
                coroutines.launch {
                    refreshing = true
                    fleet.load()
                    refreshing = false
                }
            },
            Modifier.fillMaxSize(),
        ) {
            when {
                !fleet.loaded -> Loading()
                fleet.directory.isEmpty() -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps("No machines yet") }
                else -> LazyColumn(Modifier.fillMaxSize().testTag("machines-list"), contentPadding = PaddingValues(bottom = 16.dp)) {
                    if (page.machines.isEmpty()) {
                        item {
                            Text(
                                "Nothing runs in ${project?.name ?: "this project"} yet.", Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("machines-none"),
                                style = Face.ui(14.sp), color = Palette.dim,
                            )
                        }
                    }
                    for (m in page.machines) {
                        item(key = "m:${m.id}") { MachineHead(m, fleet) }
                        val session = fleet.find(m.id) ?: continue
                        items(m.sessions, key = { "s:${m.id}:${it.id}" }) { card -> SessionBlock(session, card, open, was) }
                    }
                    if (page.quiet.isNotEmpty()) {
                        item(key = "quiet") {
                            val n = page.quiet.size
                            Row(
                                Modifier.fillMaxWidth().clickable { unfolded = !unfolded }.padding(horizontal = 16.dp, vertical = 14.dp).testTag("machines-quiet"),
                                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Icon(if (unfolded) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(18.dp), tint = Palette.dim)
                                Text(
                                    "$n ${if (n == 1) "machine" else "machines"} with nothing in ${project?.name ?: "this project"}",
                                    style = Face.ui(14.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        if (unfolded) items(page.quiet, key = { "q:${it.id}" }) { MachineHead(it, fleet, quiet = true) }
                    }
                }
            }
        }
    }
}

/** A machine's heading: whose it is, and whether it's online. An offline machine can't be opened. */
@Composable
private fun MachineHead(m: MachineSection, fleet: HomeFleet, quiet: Boolean = false) {
    val nav = LocalNav.current
    val wide = LocalWide.current
    val prefs = prefs()
    val machine = fleet.directory.firstOrNull { it.id == m.id } ?: return
    Row(
        Modifier.fillMaxWidth()
            .then(
                if (m.online) {
                    Modifier.clickable {
                        prefs.edit().putString(LAST_MACHINE, m.id).apply()
                        nav.show(wide, Dest.MachineD(machine))
                    }
                } else Modifier,
            )
            .padding(start = 16.dp, end = 16.dp, top = if (quiet) 8.dp else 16.dp, bottom = 8.dp).testTag("machine-${m.name}"),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Dot(if (m.online) Tone.OK else Tone.MUTED, 8)
        Text(
            m.name, Modifier.weight(1f, fill = false), style = Face.ui(if (quiet) 14.sp else 16.sp, FontWeight.SemiBold),
            color = if (m.online) Palette.fg else Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        when {
            m.cloud -> Caps("cloud")
            m.owner != null -> Caps("${m.owner}'s")
        }
        Spacer(Modifier.weight(1f))
        if (!m.online) Caps("offline")
    }
}

/** "Needs you", as a small chip in its tone; with a count when more than one. */
@Composable
private fun NeedsChip(n: Int = 1) {
    val tone = Standing.NEEDS.tone
    Text(
        if (n > 1) "${NEEDS_YOU.uppercase()} · $n" else NEEDS_YOU.uppercase(),
        Modifier.border(1.dp, tone.color.copy(alpha = 0.5f), CircleShape).padding(horizontal = 6.dp, vertical = 2.dp),
        style = Face.mono(9.sp, FontWeight.Bold).copy(letterSpacing = 0.6.sp), color = tone.color, maxLines = 1,
    )
}

private fun stateTone(state: String) = when (state) {
    "needs_input" -> Standing.NEEDS.tone
    "working" -> Standing.WORKING.tone
    else -> Tone.MUTED
}

/**
 * A session's card: its name (which opens it), then its panes as one-line
 * rows, under their tab's name when it has more than one. On a phone a row
 * opens its pane full screen; on a tablet it lights the pane in the live tab
 * beside. [open] is what the tablet shows; [was], where you were.
 */
@Composable
private fun SessionBlock(m: MachineSession, card: SessionCard, open: Dest.Session?, was: KeptPane?) {
    val nav = LocalNav.current
    val wide = LocalWide.current
    val shown = open != null && open.machine == m.machine.id && open.session == card.id
    val shape = RoundedCornerShape(10.dp)
    Column(
        Modifier.padding(horizontal = 12.dp, vertical = 4.dp).fillMaxWidth().clip(shape)
            .background(Palette.surface).border(1.dp, if (shown) Palette.accent.copy(alpha = 0.6f) else Palette.raised, shape),
    ) {
        Row(
            Modifier.fillMaxWidth().clickable { nav.show(wide, Dest.Session(m.machine.id, card.id)) }.padding(horizontal = 12.dp, vertical = 10.dp)
                .testTag("session-${card.name}"),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Dot(stateTone(card.state))
            Text(card.name.ifEmpty { "Session" }, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (card.needsYou > 0) NeedsChip(card.needsYou)
        }
        for (t in card.tabs) {
            if (card.tabs.size > 1) {
                val name = m.tabs.firstOrNull { it.id == t.id }?.let(m::name) ?: t.name ?: "Tab"
                Caps(name, Modifier.padding(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 2.dp))
            }
            for (id in t.panes) {
                val p = m.pane(id) ?: continue
                PaneLine(m, p, lit = shown && open?.lit == id, was = was != null && was.machine == m.machine.id && was.pane == id) {
                    // An agent block has no terminal: its session shows it, lit.
                    if (wide || p.isAgent) nav.show(wide, Dest.Session(m.machine.id, card.id, t.id, id)) else nav.push(Dest.Terminal(m.machine.id, id, t.id))
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

/** A pane in one line: what it does, and Needs you when it does. */
@Composable
private fun PaneLine(m: MachineSession, p: PaneRow, lit: Boolean, was: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(if (lit) Palette.accent.copy(alpha = 0.14f) else Color.Transparent).clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 7.dp).testTag("pane-${p.id}"),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Dot(m.standing(p).tone, 6)
        Text(m.conversations[p.id] ?: p.name, Modifier.weight(1f), style = Face.ui(14.sp), color = if (lit) Palette.fg else Palette.subtle, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (was) Caps("where you were", color = Palette.accent)
        if (p.needsYou) NeedsChip()
    }
}

/** A tablet's detail before anything is picked: the pane you were last in, live; else a word to pick one. */
@Composable
private fun WhereYouWere(api: Api, fleet: HomeFleet, home: HomeModel) {
    val project = LocalScope.current.current
    val was = project?.let { prefs().keptPane(it.id) }
    val card = was?.let { w -> fleet.find(w.machine)?.cards(project?.shown)?.firstOrNull { c -> c.tabs.any { it.id == w.tab } } }
    if (was == null || card == null) {
        Pick(AppTab.MACHINES)
        return
    }
    SessionScreen(api, Dest.Session(was.machine, card.id, was.tab, was.pane), fleet, home, label = "Where you were")
}

/** The machine's link: home's, when it keeps one, else one for this screen. */
@Composable
private fun sessionFor(api: Api, machine: Machine, fleet: HomeFleet): MachineSession {
    val kept = fleet.find(machine.id)
    val own = remember(machine.id) { if (kept == null) MachineSession(api, machine) else null }
    DisposableEffect(own) { onDispose { own?.disconnect() } }
    return kept ?: own!!
}

/** A machine: its sessions in the current project, and a new terminal on it. */
@Composable
fun MachineScreen(api: Api, machine: Machine, fleet: HomeFleet) {
    val nav = LocalNav.current
    val project = LocalScope.current.current
    val session = sessionFor(api, machine, fleet)
    val coroutines = rememberCoroutineScope()
    var problem by remember { mutableStateOf<String?>(null) }
    val was = project?.let { prefs().keptPane(it.id) }
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
        // Only the project's sessions, as the core's rule keeps them (Arugula #1352).
        val cards = session.cards(project?.shown)
        val needs = cards.sumOf { it.needsYou }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(vertical = 12.dp)) {
            item {
                val n = cards.size
                Caps(
                    (if (n == 1) "1 session" else "$n sessions") + if (needs > 0) " · $needs need you" else "",
                    Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("machine-count"),
                )
            }
            if (cards.isEmpty()) {
                item {
                    Text(
                        "Nothing of ${project?.name ?: "this project"} runs here. A new terminal starts something.",
                        Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = Face.ui(14.sp), color = Palette.dim,
                    )
                }
            }
            items(cards, key = { it.id }) { SessionBlock(session, it, null, was) }
        }
    }
}

/**
 * A session, a tab at a time: on a phone its panes stacked, each live with
 * its last lines and what it asks answered in place; on a tablet the tab as
 * it's laid out. Tapping a pane opens it full screen, to type in. [label]
 * says why it shows (where you were) when nothing was picked.
 */
@Composable
fun SessionScreen(api: Api, d: Dest.Session, fleet: HomeFleet, home: HomeModel, label: String? = null) {
    val wide = LocalWide.current
    val project = LocalScope.current.current
    val prefs = prefs()
    val m = fleet.find(d.machine)
    val card = m?.cards(project?.shown)?.firstOrNull { it.id == d.session }
    var picked by remember(d) { mutableStateOf(d.tab) }
    // A pane picked on a tablet is where you are now.
    LaunchedEffect(d) { if (label == null && d.tab != null && d.lit != null) prefs.keep(project?.id, KeptPane(d.machine, d.tab, d.lit)) }
    if (m != null) LaunchedEffect(m) { m.connect() }

    Column(Modifier.fillMaxSize().testTag("session")) {
        DetailBar(card?.name?.ifEmpty { null } ?: "Session", back = label == null) { Caps(label ?: m?.machine?.name ?: "") }
        m?.problem?.let { Text(it, Modifier.padding(horizontal = 16.dp, vertical = 4.dp), style = Face.mono(12.sp), color = Palette.danger) }
        if (m == null || card == null) {
            if (m != null && !m.connected && m.problem == null) {
                Loading()
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps(if (m == null) "The machine is offline" else "This session isn't running") }
            }
            return@Column
        }
        // The tab asked for, else the one used last.
        val tab = card.tabs.firstOrNull { it.id == picked } ?: card.tabs.maxByOrNull { t -> t.panes.maxOfOrNull { m.pane(it)?.lastMs ?: 0 } ?: 0 } ?: return@Column
        if (card.tabs.size > 1) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                for (t in card.tabs) {
                    val on = t.id == tab.id
                    Row(
                        Modifier.clip(CircleShape).background(if (on) Palette.accent else Palette.raised).clickable { picked = t.id }
                            .padding(horizontal = 12.dp, vertical = 6.dp).testTag("session-tab-${t.id}"),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Text(m.tabs.firstOrNull { it.id == t.id }?.let(m::name) ?: t.name ?: "Tab", style = Face.ui(13.sp, FontWeight.Medium), color = if (on) Palette.void else Palette.fg, maxLines = 1)
                        if (t.panes.any { m.pane(it)?.needsYou == true }) Dot(if (on) Tone.MUTED else Standing.NEEDS.tone, 6)
                    }
                }
            }
        }
        val layout = m.tabs.firstOrNull { it.id == tab.id }
        if (layout == null) {
            Loading()
            return@Column
        }
        if (wide) LiveTab(m, layout, tab, d.lit) else LiveStack(api, m, layout, tab, d.lit, home)
    }
}

/**
 * The terminals of [layout]'s panes, each at its size on the machine, fed
 * while they show: the tab watched whole, so nobody's panes resize.
 */
@Composable
private fun watching(m: MachineSession, layout: TabLayout): Map<Int, Term> {
    val terms = remember(m, layout) {
        layout.cells.filter { (it.kind ?: "terminal") == "terminal" }.associate { it.pane to Term(it.cols.coerceAtLeast(2), it.rows.coerceAtLeast(1)) }
    }
    DisposableEffect(m, terms) {
        terms.forEach { (p, t) -> m.terms[p] = t }
        m.watch(layout)
        onDispose {
            m.unwatch(layout)
            for ((p, t) in terms) {
                if (m.terms[p] === t) m.terms.remove(p)
                t.close()
            }
        }
    }
    return terms
}

/** Open a pane full screen, to type in it; an agent block has no terminal. */
private fun Nav.openPane(m: MachineSession, p: PaneRow) {
    if (p.type != "agent") push(Dest.Terminal(m.machine.id, p.id, p.tab))
}

/** A phone's session tab: its panes stacked, each live with its last lines, and what it asks answered in place. */
@Composable
private fun LiveStack(api: Api, m: MachineSession, layout: TabLayout, tab: SessionTab, lit: Int?, home: HomeModel) {
    val nav = LocalNav.current
    val terms = watching(m, layout)
    val list = rememberLazyListState()
    LaunchedEffect(lit, tab.id) { tab.panes.indexOf(lit).takeIf { it >= 0 }?.let { list.scrollToItem(it) } }
    LazyColumn(Modifier.fillMaxSize(), list, contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        items(tab.panes, key = { it }) { id ->
            val p = m.pane(id) ?: return@items
            val cell = layout.cells.firstOrNull { it.pane == id }
            ToneCard(m.standing(p).tone, Modifier.testTag("stacked-$id"), glow = id == lit) {
                Row(
                    Modifier.fillMaxWidth().clickable { nav.openPane(m, p) },
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text(m.conversations[p.id] ?: p.name, Modifier.weight(1f), style = Face.ui(14.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    p.program?.let { Caps(it) }
                    if (p.needsYou) NeedsChip()
                }
                val term = terms[id]
                if (term != null && cell != null) {
                    TermView(
                        term, m.frames, cell.cols, PREVIEW_LINES,
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp)).background(Palette.void).clickable { nav.openPane(m, p) }.testTag("live-$id"),
                    )
                } else {
                    p.headline?.takeIf { it.isNotEmpty() }?.let { Text(it, style = Face.ui(13.sp), color = Palette.subtle, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                }
                if (p.needsYou) NeedAnswer(paneNeed(m, p), api, home) {}
            }
        }
    }
}

/** How many of a stacked pane's last lines show. */
private const val PREVIEW_LINES = 10

/** A tablet's session tab, as the machine lays it out, live; the pane picked lit. */
@Composable
private fun LiveTab(m: MachineSession, layout: TabLayout, tab: SessionTab, lit: Int?) {
    val nav = LocalNav.current
    val terms = watching(m, layout)
    val cell = termCell()
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxSize().padding(12.dp).testTag("live-tab")) {
        val (w, h) = with(density) { maxWidth.toPx() to maxHeight.toPx() }
        val s = minOf(w / (layout.cols.coerceAtLeast(1) * cell.width), h / (layout.rows.coerceAtLeast(1) * cell.height))
        for (c in layout.cells) {
            if (c.pane !in tab.panes) continue
            val p = m.pane(c.pane) ?: continue
            val edge = when {
                c.pane == lit -> Palette.accent
                p.needsYou -> Standing.NEEDS.tone.color.copy(alpha = 0.6f)
                else -> Palette.raised
            }
            with(density) {
                Box(
                    Modifier.offset((c.x * cell.width * s).toDp(), (c.y * cell.height * s).toDp())
                        .size((c.cols * cell.width * s).toDp(), (c.rows * cell.height * s).toDp())
                        .border(if (c.pane == lit) 2.dp else 1.dp, edge).background(Palette.void).clickable { nav.openPane(m, p) }.testTag("live-${c.pane}"),
                ) {
                    val term = terms[c.pane]
                    if (term != null) {
                        TermView(term, m.frames, c.cols, null, Modifier.fillMaxSize())
                    } else {
                        Text(m.conversations[p.id] ?: p.name, Modifier.padding(8.dp), style = Face.ui(13.sp), color = Palette.subtle)
                    }
                    if (p.needsYou) Box(Modifier.align(Alignment.TopEnd).padding(4.dp)) { NeedsChip() }
                }
            }
        }
    }
}

/** A terminal cell's size in the mono face, in pixels. */
@Composable
private fun termCell(): Size {
    val measurer = rememberTextMeasurer()
    return remember(measurer) { measurer.measure("M", TERM_STYLE).size.let { Size(it.width.toFloat(), it.height.toFloat()) } }
}

private val TERM_STYLE get() = Face.mono(13.sp)

/**
 * A pane's screen, drawn small to fit: its [cols] across the width, and
 * [lines] of it (its last, down to the cursor) or the whole screen.
 */
@Composable
private fun TermView(term: Term, frames: Int, cols: Int, lines: Int?, modifier: Modifier) {
    val rows = remember(term) { mutableMapOf<Int, TermRow>() }
    var frame by remember(term) { mutableStateOf<Frame?>(null) }
    var drawn by remember(term) { mutableIntStateOf(0) }
    val measurer = rememberTextMeasurer()
    val cell = termCell()
    LaunchedEffect(term, frames) {
        val f = term.render()
        if (f.dirty == Frame.FULL) rows.clear()
        for (r in f.changed) rows[r.y] = r
        frame = f
        drawn++
    }
    val sized = if (lines == null) modifier else modifier.aspectRatio(cols.coerceAtLeast(1) * cell.width / (lines * cell.height))
    Canvas(sized.clipToBounds()) {
        drawn.let { }
        val f = frame ?: return@Canvas
        if (f.bg != Frame.DEFAULT_BG) drawRect(rgb(f.bg))
        val s = if (lines == null) minOf(size.width / (f.cols * cell.width), size.height / (f.rows * cell.height)) else size.width / (f.cols * cell.width)
        // Its last lines: down to the cursor, or the last row with something on it.
        val first = if (lines == null) 0 else {
            val written = rows.filter { (y, r) -> y < f.rows && r.text.isNotBlank() }.keys.maxOrNull() ?: 0
            (maxOf(written, if (f.cursorVisible) f.cursorY else 0) - lines + 1).coerceAtLeast(0)
        }
        withTransform({
            scale(s, s, Offset.Zero)
            translate(0f, -first * cell.height)
        }) { drawTerm(f, rows, measurer, TERM_STYLE, cell.width, cell.height) }
    }
}

private fun rgb(c: Int) = Color(0xFF000000.toInt() or (c and 0xFFFFFF))

/** The rows that have something, in their runs' colors, and the cursor. */
private fun DrawScope.drawTerm(f: Frame, rows: Map<Int, TermRow>, measurer: TextMeasurer, style: TextStyle, cellW: Float, cellH: Float) {
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
    val cell = termCell()
    val cellW = cell.width
    val cellH = cell.height
    val project = LocalScope.current.current?.id
    val prefs = prefs()

    // The pane you're in: where you were, next time.
    LaunchedEffect(d) { prefs.keep(project, KeptPane(d.machine, d.tab, d.pane)) }

    DisposableEffect(session, term) {
        session.terms[d.pane] = term
        onDispose {
            if (session.terms[d.pane] === term) session.terms.remove(d.pane)
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
                drawTerm(f, rows, measurer, TERM_STYLE, cellW, cellH)
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
