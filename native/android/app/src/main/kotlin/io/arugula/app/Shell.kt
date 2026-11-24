// One navigation: the web's five places always on the bar (#1262, Arugula
// #1362) — the Overview, the project's Boards and Docs, Machines (the
// project's sessions on each machine, Arugula #1352, #1358) and Chat — each
// with its own back stack, as the iPhone's HomeView.swift. On a tablet the
// same places are a navigation rail, with the bell and your picture at its
// foot (Arugula #1366). The bar stays on every screen, whatever is open.
// Every place is headed by the project's menu and New, and on a phone the
// bell and your picture (TabHeading, Account.kt); the project's issues open
// from its menu and from a board's rows. A link into the app (`arugula://…`,
// Arugula #1365) opens a place, the Inbox, or an entry in it.

package io.arugula.app

import android.content.Context
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.material3.VerticalDivider
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material.icons.outlined.ViewWeek
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.launch

/**
 * The app's places, always on its bar, as the web's rail has them (Arugula
 * #1362): the Overview, Boards and Docs lead, then Machines and Chat. The
 * Inbox and the account are in every place's heading on a phone, at the
 * rail's foot on a tablet (Arugula #1366), never among the places.
 */
enum class AppTab(val label: String, val icon: ImageVector) {
    OVERVIEW("Overview", Icons.Outlined.GridView),
    BOARDS("Boards", Icons.Outlined.ViewWeek),
    DOCS("Docs", Icons.Outlined.Description),
    MACHINES("Machines", Icons.Outlined.Terminal),
    CHAT("Chat", Icons.Outlined.ChatBubbleOutline),
}

/** Something a tab opens over its list. */
sealed interface Dest {
    /** A board's thread: its lane bar and lead over the conversation. */
    data class BoardThread(val board: Board) : Dest

    /** A board's lanes. */
    data class Lanes(val board: Board) : Dest

    /** A room, or one message's thread in it. */
    data class Room(val room: ChatRoom, val thread: Long? = null, val title: String? = null) : Dest

    /** A thread found by its object (a New idea's issue). */
    data class ObjectThread(val target: String, val title: String) : Dest

    /** An issue by its id (`iss_…`), or by its number in [project]. */
    data class IssueRef(val id: String, val project: String? = null) : Dest

    /** A project Doc, in its page's web view; [over], another project's, over everything. */
    data class Doc(val ref: DocRef, val over: Boolean = false) : Dest

    /** The current project's open issues. */
    data object Issues : Dest

    data class MachineD(val machine: Machine) : Dest

    data class Terminal(val machine: String, val pane: Int, val tab: Int) : Dest

    /** A machine's session, a tab at a time: [tab] first, else its last; [lit], the pane picked in it. */
    data class Session(val machine: String, val session: Int, val tab: Int? = null, val lit: Int? = null) : Dest
}

/** What every screen inside the shell can reach. */
class Nav(val tab: () -> AppTab, private val stacks: Map<AppTab, SnapshotStateList<Dest>>, val select: (AppTab) -> Unit) {
    fun push(d: Dest) = stacks.getValue(tab()).add(d)
    fun pop() = stacks.getValue(tab()).removeLastOrNull()

    /** Show [d] in place of whatever the tab had open: a tablet's detail, beside its list. */
    fun replace(d: Dest) = stacks.getValue(tab()).apply { clear(); add(d) }

    /** Open [d] on [t]'s stack, over its list, and go there. */
    fun open(t: AppTab, d: Dest) {
        stacks.getValue(t).apply { clear(); add(d) }
        select(t)
    }
}

val LocalNav = staticCompositionLocalOf<Nav> { error("no Nav") }
val LocalScope = staticCompositionLocalOf<Scope> { error("no Scope") }

/** A tablet's rail is showing: the bell and your picture are at its foot, not in the heading (Arugula #1366). */
val LocalRail = compositionLocalOf { false }

/**
 * The project the app is in, as the web's current project: the Overview,
 * Boards, Docs, Machines, Chat and its issues show what's in it. Kept on this device; the reader's
 * personal project until they pick another.
 */
class Scope(context: Context) {
    var projects by mutableStateOf<List<Project>>(emptyList())
        private set
    var current by mutableStateOf<Project?>(null)
        private set
    var me by mutableStateOf<Me?>(null)
        private set
    var problem by mutableStateOf<String?>(null)
        private set
    private val prefs = context.getSharedPreferences("arugula", Context.MODE_PRIVATE)

    suspend fun load(api: Api) {
        try {
            val p = Projects.of(api.get("/api/projects"))
            projects = p.projects
            me = p.me
            val kept = prefs.getString(KEY, null)
            current = p.projects.firstOrNull { it.id == kept } ?: p.projects.firstOrNull { it.team == null } ?: p.projects.firstOrNull()
            problem = null
        } catch (e: Exception) {
            problem = describe(e)
        }
    }

    fun pick(p: Project) {
        current = p
        prefs.edit().putString(KEY, p.id).apply()
    }

    /** Chat's rooms: the project's team's, or every room in the personal project, which has no team. */
    fun showsRoom(room: ChatRoom): Boolean {
        val team = current?.team ?: return true
        return room.team == team.id
    }

    private companion object {
        const val KEY = "current-project"
    }
}

@Composable
fun Shell(api: Api, model: AppModel) {
    val context = LocalContext.current
    val scope = remember(api) { Scope(context) }
    val live = remember(api) { LiveChat(api) }
    val home = remember(api) { HomeModel(api) }
    val fleet = remember(api) { HomeFleet(api) }
    var tab by rememberSaveable { mutableStateOf(AppTab.OVERVIEW) }
    val stacks = remember(api) { AppTab.entries.associateWith { mutableStateListOf<Dest>() } }
    val nav = remember(api) { Nav({ tab }, stacks) { tab = it } }
    val stack = stacks.getValue(tab)
    val coroutines = rememberCoroutineScope()
    val snack = remember { SnackbarHostState() }
    /** New, from any place's heading, and the bell's Inbox. */
    var making by remember { mutableStateOf(false) }
    var inbox by remember { mutableStateOf(false) }
    val heading = remember(api) { Heading(api, model, makeNew = { making = true }, showInbox = { inbox = true }) }

    LaunchedEffect(api) {
        live.start()
        // The projects, again until they come (offline as the app opened).
        scope.load(api)
        while (scope.current == null) {
            kotlinx.coroutines.delay(5_000)
            scope.load(api)
        }
    }
    // Another project: nothing of the last one stays open.
    LaunchedEffect(scope.current?.id) {
        for (t in AppTab.entries) stacks.getValue(t).clear()
    }
    // The bell's count (Arugula #1365): as the app opens or comes back, and in another project.
    val lifecycle = LocalLifecycleOwner.current
    LaunchedEffect(heading, scope.current?.id) {
        val project = scope.current?.id ?: return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) { heading.inbox.load(project) }
    }
    // A link into the app (Arugula #1365), once the projects are in: a
    // place, the Inbox, or an entry in it, seen as it opens.
    LaunchedEffect(model.link, scope.current?.id) {
        val link = model.link ?: return@LaunchedEffect
        if (scope.current == null) return@LaunchedEffect
        model.link = null
        val uri = Uri.parse(link)
        val host = uri.host.orEmpty()
        AppTab.entries.firstOrNull { it.name.equals(host, ignoreCase = true) }?.let {
            tab = it
            return@LaunchedEffect
        }
        if (host != "inbox") return@LaunchedEffect
        val item = uri.pathSegments.firstOrNull()
        if (item == null) {
            inbox = true
            return@LaunchedEffect
        }
        // A native issue on its page here; anything else as its entry opens, a run on the platform's page.
        if (item.startsWith("iss_")) nav.push(Dest.IssueRef(item))
        heading.inbox.later {
            markItem(item)
            if (item.startsWith("iss_")) return@later
            val e = entries?.firstOrNull { it.item == item }
            when {
                e != null -> openEntry(context, heading, nav, e)
                item.startsWith("run_") -> openOnPlatform(context, model.url, "/runs/$item")
                else -> inbox = true
            }
        }
    }
    // Back: off the place's stack, then to the Overview, then out of the app.
    BackHandler(enabled = stack.isNotEmpty() || tab != AppTab.OVERVIEW) {
        if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) else tab = AppTab.OVERVIEW
    }
    // The place again: back to its list.
    val go: (AppTab) -> Unit = { t -> if (tab == t) stacks.getValue(t).clear() else tab = t }

    androidx.compose.runtime.CompositionLocalProvider(LocalNav provides nav, LocalScope provides scope, LocalHeading provides heading) {
        BoxWithConstraints(Modifier.fillMaxSize()) {
            // A tablet: the same places in a rail down the left, as the iPad's
            // sidebar; a wide one, a place's list beside what it opened.
            val rail = maxWidth >= 600.dp
            val wide = maxWidth >= 840.dp
            CompositionLocalProvider(LocalRail provides rail) {
                Scaffold(
                    containerColor = Palette.void,
                    snackbarHost = { SnackbarHost(snack) },
                    bottomBar = { if (!rail) PlacesBar(tab, go) },
                ) { padding ->
                    Row(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)) {
                        if (rail) {
                            PlacesRail(tab, go, heading)
                            VerticalDivider(color = Palette.raised)
                        }
                        Box(Modifier.weight(1f).fillMaxHeight().imePadding()) {
                            val top = stack.lastOrNull()
                            val list: @Composable () -> Unit = {
                                when (tab) {
                                    AppTab.OVERVIEW -> HomeScreen(api, home, fleet, live)
                                    AppTab.BOARDS -> BoardsScreen(api)
                                    AppTab.DOCS -> DocsScreen(api, stack.filterIsInstance<Dest.Doc>().lastOrNull()?.ref)
                                    AppTab.MACHINES -> {}
                                    AppTab.CHAT -> ChatRoomsScreen(api, live)
                                }
                            }
                            // A wide tablet, as the iPad: the place's list beside what
                            // it opened. A terminal takes the whole width, and so does
                            // another project's Doc. Machines lays itself out
                            // (ListDetailPaneScaffold).
                            if (tab == AppTab.MACHINES && top !is Dest.Terminal) {
                                MachinesScreen(api, fleet, home, top) { Destination(api, it, home, fleet, live) }
                            } else if (wide && top !is Dest.Terminal && (top as? Dest.Doc)?.over != true) {
                                Row(Modifier.fillMaxSize()) {
                                    Box(Modifier.width(380.dp).fillMaxHeight()) { list() }
                                    VerticalDivider(color = Palette.raised)
                                    Box(Modifier.weight(1f).fillMaxHeight()) {
                                        if (top != null) Destination(api, top, home, fleet, live) else Pick(tab)
                                    }
                                }
                            } else if (top != null) {
                                Destination(api, top, home, fleet, live)
                            } else {
                                list()
                            }
                        }
                    }
                }
            }
        }
        // New over whichever place is open; what it makes opens on the Overview, which lists it.
        if (making) {
            NewSheet(api, home, fleet, onDismiss = { making = false }, toast = { t -> coroutines.launch { snack.showSnackbar(t) } }, made = {
                coroutines.launch { scope.current?.let { home.load(it.id) } }
            })
        }
        if (inbox) InboxSheet(heading) { inbox = false }
    }
}

/** The places on a phone: the bar along the bottom. */
@Composable
private fun PlacesBar(tab: AppTab, go: (AppTab) -> Unit) {
    NavigationBar(containerColor = Palette.void, tonalElevation = 0.dp) {
        for (t in AppTab.entries) {
            NavigationBarItem(
                selected = tab == t,
                onClick = { go(t) },
                icon = { Icon(t.icon, null) },
                label = { Text(t.label, style = Face.ui(12.sp, FontWeight.Medium)) },
                modifier = Modifier.testTag("tab-${t.label}"),
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = Palette.void,
                    indicatorColor = Palette.accent,
                    selectedTextColor = Palette.accent,
                    unselectedIconColor = Palette.dim,
                    unselectedTextColor = Palette.dim,
                ),
            )
        }
    }
}

/**
 * The places on a tablet: a rail down the left, centered, with the bell and
 * your picture at its foot (Arugula #1366), as the web's rail. On a short
 * screen (a phone on its side) the places scroll; the foot stays.
 */
@Composable
private fun PlacesRail(tab: AppTab, go: (AppTab) -> Unit, heading: Heading) {
    NavigationRail(containerColor = Palette.void) {
        BoxWithConstraints(Modifier.weight(1f)) {
            val room = maxHeight
            Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                Column(Modifier.heightIn(min = room), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    RailPlaces(tab, go)
                }
            }
        }
        Column(Modifier.padding(top = 8.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            InboxBell(heading, Modifier.testTag("rail-inbox"))
            AccountMenu(heading, tag = "rail-account")
        }
    }
}

@Composable
private fun RailPlaces(tab: AppTab, go: (AppTab) -> Unit) {
    for (t in AppTab.entries) {
        NavigationRailItem(
            selected = tab == t,
            onClick = { go(t) },
            icon = { Icon(t.icon, null) },
            label = { Text(t.label, style = Face.ui(12.sp, FontWeight.Medium)) },
            modifier = Modifier.testTag("tab-${t.label}"),
            colors = NavigationRailItemDefaults.colors(
                selectedIconColor = Palette.void,
                indicatorColor = Palette.accent,
                selectedTextColor = Palette.accent,
                unselectedIconColor = Palette.dim,
                unselectedTextColor = Palette.dim,
            ),
        )
    }
}

/** What a tablet's detail shows before anything is picked. */
@Composable
fun Pick(tab: AppTab) {
    val words = when (tab) {
        AppTab.OVERVIEW -> "Pick a conversation"
        AppTab.BOARDS -> "Pick a board"
        AppTab.CHAT -> "Pick a room"
        AppTab.DOCS -> "Pick a Doc"
        else -> "Pick a session"
    }
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically), horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(tab.icon, null, Modifier.size(34.dp), tint = Palette.muted)
        Caps(words)
    }
}

@Composable
private fun Destination(api: Api, d: Dest, home: HomeModel, fleet: HomeFleet, live: LiveChat) {
    when (d) {
        is Dest.BoardThread -> BoardThreadScreen(api, d.board, home, fleet, live)
        is Dest.Lanes -> BoardScreen(api, d.board)
        is Dest.Room -> ChatRoomScreen(api, d.room, live, title = d.title, thread = d.thread)
        is Dest.ObjectThread -> ObjectThreadScreen(api, d.target, d.title, live)
        is Dest.IssueRef -> IssueScreen(api, d.id, d.project)
        is Dest.Doc -> DocScreen(api, d.ref)
        Dest.Issues -> IssuesScreen(api)
        is Dest.MachineD -> MachineScreen(api, d.machine, fleet)
        is Dest.Terminal -> TerminalScreen(api, d, fleet)
        is Dest.Session -> SessionScreen(api, d, fleet, home)
    }
}

/**
 * The heading at the top of every place's list, as the web phone's top bar
 * (Arugula #1362): the current project, as a menu of every project and its
 * issues, with whose it is; then New, the bell (the Inbox, with its count,
 * Arugula #1365) and your picture (Settings, Send feedback, Sign out). On a
 * tablet the bell and your picture are at the rail's foot instead (Arugula
 * #1366). The place's own name is already on the bar. The three buttons take
 * 120dp, so the four fit on a 360dp phone; a long project name truncates,
 * the buttons don't. [title] is what it says before the projects come.
 */
@Composable
fun TabHeading(title: String) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    val heading = LocalHeading.current
    val rail = LocalRail.current
    var open by remember { mutableStateOf(false) }
    // The bell's count again as a place comes up, unless it was just read.
    LaunchedEffect(heading, scope.current?.id) {
        val project = scope.current?.id ?: return@LaunchedEffect
        heading?.inbox?.refresh(project)
    }
    Column(Modifier.fillMaxWidth().background(Palette.void).padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Box(Modifier.weight(1f)) {
                Row(Modifier.clickable { open = true }.testTag("project-switcher"), verticalAlignment = Alignment.CenterVertically) {
                    Text((scope.current?.name ?: title).uppercase(), Modifier.weight(1f, fill = false), style = Face.display(30.sp), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Icon(Icons.Outlined.KeyboardArrowDown, null, tint = Palette.accent)
                }
                DropdownMenu(open, { open = false }, containerColor = Palette.surface) {
                    Caps("Projects", Modifier.padding(horizontal = 12.dp, vertical = 6.dp))
                    for (p in scope.projects) {
                        DropdownMenuItem(
                            text = { Text(p.name, style = Face.ui(15.sp)) },
                            leadingIcon = { if (p.id == scope.current?.id) Icon(Icons.Outlined.Check, null, tint = Palette.accent) else Spacer(Modifier.width(24.dp)) },
                            onClick = {
                                open = false
                                scope.pick(p)
                            },
                        )
                    }
                    HorizontalDivider(color = Palette.raised)
                    DropdownMenuItem(
                        text = { Text("Issues", style = Face.ui(15.sp)) },
                        onClick = {
                            open = false
                            nav.push(Dest.Issues)
                        },
                        modifier = Modifier.testTag("project-issues"),
                    )
                }
            }
            if (heading != null) {
                HeadingButton(Icons.Outlined.Add, "New", Modifier.testTag("heading-new"), primary = true, onClick = heading.makeNew)
                if (!rail) {
                    InboxBell(heading, Modifier.testTag("heading-inbox"))
                    AccountMenu(heading)
                }
            }
        }
        scope.current?.team?.let { Caps("${it.name} · ${it.role}") }
        scope.problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger, maxLines = 2) }
    }
}

/** A detail's bar: back (unless it's what shows before anything is picked), its title, and what goes on the right. */
@Composable
fun DetailBar(title: String, back: Boolean = true, trailing: @Composable () -> Unit = {}) {
    val nav = LocalNav.current
    Row(Modifier.fillMaxWidth().background(Palette.void).padding(start = if (back) 0.dp else 16.dp, end = 12.dp).heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
        if (back) IconButton({ nav.pop() }, Modifier.testTag("back")) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back", tint = Palette.fg) }
        Text(title, Modifier.weight(1f), style = Face.ui(17.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) { trailing() }
    }
    HorizontalDivider(color = Palette.raised)
}
