// One navigation: the tabs always on the bar (#1262) — Home, the project's
// Boards and Chat, the account's Machines, and You — each with its own back
// stack, as the iPhone's HomeView.swift. The bar stays on every screen,
// whatever is open. The project's issues open from its menu and from a
// board's rows.

package io.arugula.app

import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.AccountCircle
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Home
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
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
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

/** The app's tabs, always on its bar: home first. */
enum class AppTab(val label: String, val icon: ImageVector) {
    HOME("Home", Icons.Outlined.Home),
    BOARDS("Boards", Icons.Outlined.ViewWeek),
    CHAT("Chat", Icons.Outlined.ChatBubbleOutline),
    MACHINES("Machines", Icons.Outlined.Terminal),
    YOU("You", Icons.Outlined.AccountCircle),
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

    data class IssueRef(val id: String) : Dest

    /** The current project's open issues. */
    data object Issues : Dest

    data class MachineD(val machine: Machine) : Dest

    data class Terminal(val machine: String, val pane: Int, val tab: Int) : Dest
}

/** What every screen inside the shell can reach. */
class Nav(val tab: () -> AppTab, private val stacks: Map<AppTab, SnapshotStateList<Dest>>, val select: (AppTab) -> Unit) {
    fun push(d: Dest) = stacks.getValue(tab()).add(d)
    fun pop() = stacks.getValue(tab()).removeLastOrNull()

    /** Open [d] on [t]'s stack, over its list, and go there. */
    fun open(t: AppTab, d: Dest) {
        stacks.getValue(t).apply { clear(); add(d) }
        select(t)
    }
}

val LocalNav = staticCompositionLocalOf<Nav> { error("no Nav") }
val LocalScope = staticCompositionLocalOf<Scope> { error("no Scope") }

/**
 * The project the app is in, as the web's current project: Home, Boards,
 * Chat and its issues show what's in it. Kept on this device; the reader's
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
    var tab by rememberSaveable { mutableStateOf(AppTab.HOME) }
    val stacks = remember(api) { AppTab.entries.associateWith { mutableStateListOf<Dest>() } }
    val nav = remember(api) { Nav({ tab }, stacks) { tab = it } }
    val stack = stacks.getValue(tab)

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
        for (t in listOf(AppTab.HOME, AppTab.BOARDS, AppTab.CHAT)) stacks.getValue(t).clear()
    }
    // Back: off the tab's stack, then to Home, then out of the app.
    BackHandler(enabled = stack.isNotEmpty() || tab != AppTab.HOME) {
        if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) else tab = AppTab.HOME
    }

    androidx.compose.runtime.CompositionLocalProvider(LocalNav provides nav, LocalScope provides scope) {
        Scaffold(
            containerColor = Palette.void,
            bottomBar = {
                NavigationBar(containerColor = Palette.void, tonalElevation = 0.dp) {
                    for (t in AppTab.entries) {
                        NavigationBarItem(
                            selected = tab == t,
                            onClick = {
                                // The tab again: back to its list.
                                if (tab == t) stacks.getValue(t).clear() else tab = t
                            },
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
            },
        ) { padding ->
            Box(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding).imePadding()) {
                val top = stack.lastOrNull()
                if (top != null) {
                    Destination(api, top, home, fleet, live)
                } else {
                    when (tab) {
                        AppTab.HOME -> HomeScreen(api, home, fleet, live)
                        AppTab.BOARDS -> BoardsScreen(api)
                        AppTab.CHAT -> ChatRoomsScreen(api, live)
                        AppTab.MACHINES -> MachinesScreen(api, fleet)
                        AppTab.YOU -> YouScreen(model)
                    }
                }
            }
        }
    }
}

@Composable
private fun Destination(api: Api, d: Dest, home: HomeModel, fleet: HomeFleet, live: LiveChat) {
    when (d) {
        is Dest.BoardThread -> BoardThreadScreen(api, d.board, home, fleet, live)
        is Dest.Lanes -> BoardScreen(api, d.board)
        is Dest.Room -> ChatRoomScreen(api, d.room, live, title = d.title, thread = d.thread)
        is Dest.ObjectThread -> ObjectThreadScreen(api, d.target, d.title, live)
        is Dest.IssueRef -> IssueScreen(api, d.id)
        Dest.Issues -> IssuesScreen(api)
        is Dest.MachineD -> MachineScreen(api, d.machine, fleet)
        is Dest.Terminal -> TerminalScreen(api, d, fleet)
    }
}

/**
 * The heading at the top of a tab's list. In a project's tabs it's the
 * current project, as a menu of every project and its issues, with whose it
 * is; Machines and You, the account's, are headed by their name. [trailing]
 * goes on the right (Home's New).
 */
@Composable
fun TabHeading(title: String, project: Boolean = false, trailing: @Composable () -> Unit = {}) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    var open by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().background(Palette.void).padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (project) {
                Box(Modifier.weight(1f, fill = false)) {
                    Row(Modifier.clickable { open = true }.testTag("project-switcher"), verticalAlignment = Alignment.CenterVertically) {
                        Text((scope.current?.name ?: title).uppercase(), style = Face.display(30.sp), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
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
            } else {
                Text(title.uppercase(), style = Face.display(30.sp), color = Palette.fg)
            }
            Spacer(Modifier.weight(1f))
            trailing()
        }
        if (project) scope.current?.team?.let { Caps("${it.name} · ${it.role}") }
        scope.problem?.let { Text(it, style = Face.mono(11.sp), color = Palette.danger, maxLines = 2) }
    }
}

/** A detail's bar: back, its title, and what goes on the right. */
@Composable
fun DetailBar(title: String, trailing: @Composable () -> Unit = {}) {
    val nav = LocalNav.current
    Row(Modifier.fillMaxWidth().background(Palette.void).padding(end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton({ nav.pop() }, Modifier.testTag("back")) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back", tint = Palette.fg) }
        Text(title, Modifier.weight(1f), style = Face.ui(17.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) { trailing() }
    }
    HorizontalDivider(color = Palette.raised)
}
