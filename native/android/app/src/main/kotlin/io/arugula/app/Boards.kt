// The current project's boards and issues, and You: as the iPhone's
// HomeView.swift shows them. A board opens to its lanes, stacked; Done shows
// its latest rows as single lines until it's opened; a row with a native
// issue opens it.

package io.arugula.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.staggeredgrid.LazyVerticalStaggeredGrid
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridCells
import androidx.compose.foundation.lazy.staggeredgrid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Something that loads: a spinner, then [content], or what went wrong with
 * Try again. Pulled down, it loads again; [every] looks again on a timer, and
 * [trigger] whenever it changes (a live stream's cue). [key] starts it over.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun <T> Loaded(key: Any?, every: Long? = null, trigger: Int = 0, load: suspend () -> T, content: @Composable (T, refresh: () -> Unit) -> Unit) {
    var value by remember(key) { mutableStateOf<T?>(null) }
    var problem by remember(key) { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    suspend fun refresh() {
        try {
            value = load()
            problem = null
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            problem = describe(e)
        }
    }
    LaunchedEffect(key) {
        refresh()
        if (every != null) while (isActive) {
            delay(every)
            refresh()
        }
    }
    LaunchedEffect(trigger) { if (trigger != 0) refresh() }
    val again: () -> Unit = {
        scope.launch {
            refreshing = true
            refresh()
            refreshing = false
        }
    }
    val v = value
    when {
        v != null -> PullToRefreshBox(refreshing, again, Modifier.fillMaxSize()) { content(v, again) }
        problem != null -> Problem(problem!!, again)
        else -> Loading()
    }
}

/** The current project's boards as cards; a board opens to its lanes. */
@Composable
fun BoardsScreen(api: Api) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    Column(Modifier.fillMaxSize()) {
        TabHeading("Boards")
        val p = scope.current ?: return@Column Loading()
        Loaded(key = p.id, load = { api.get("/api/projects/${p.id}/boards").list("boards", Board::of) }) { boards, _ ->
            if (boards.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps("No boards") }
            } else {
                LazyVerticalStaggeredGrid(
                    StaggeredGridCells.Adaptive(320.dp), Modifier.fillMaxSize(),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
                    verticalItemSpacing = 14.dp, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    items(boards, key = { it.id }) { b -> BoardCard(b, Modifier.clickable { nav.push(Dest.Lanes(b)) }.testTag("board-${b.title}")) }
                }
            }
        }
    }
}

/** A board on a project's list: its title, its lanes as one bar, and the counts. */
@Composable
private fun BoardCard(b: Board, modifier: Modifier) {
    ToneCard(Tone.ACCENT, modifier) {
        Text(b.title.uppercase(), style = Face.display(24.sp), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        b.goal?.takeIf { it.isNotEmpty() }?.let { Text(it, style = Face.ui(13.sp), color = Palette.subtle, maxLines = 2, overflow = TextOverflow.Ellipsis) }
        val p = b.progress
        if (p != null && p.total > 0) {
            LaneBar(p.done, p.review, p.doing, p.total, Modifier.padding(top = 4.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                Count(p.done, "done", Tone.OK, of = p.total)
                if (p.review > 0) Count(p.review, "review", Tone.ACCENT)
                if (p.doing > 0) Count(p.doing, "doing", Tone.WARN)
            }
        }
    }
}

/** The four lanes' tones, as the web's board has them (Next plain, Doing warn, Review accent, Done ok). */
fun laneTone(id: String) = when (id) {
    "lane_doing" -> Tone.WARN
    "lane_review" -> Tone.ACCENT
    "lane_done" -> Tone.OK
    else -> Tone.MUTED
}

private const val DONE_SHOWN = 8

/** A board: its goal, the four lanes' counts, and the lanes with their rows as cards in each lane's tone. */
@Composable
fun BoardScreen(api: Api, board: Board) {
    val nav = LocalNav.current
    var allDone by remember { mutableStateOf(false) }
    var fullGoal by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize()) {
        DetailBar(board.title)
        Loaded(key = board.id, load = { BoardDetail.of(api.get("/api/boards/${board.id}")) }) { b, _ ->
            val open: (BoardDetail.Row) -> Unit = { r -> if (r.pointer?.kind == "issue") r.pointer.id?.let { nav.push(Dest.IssueRef(it)) } }
            LazyColumn(Modifier.fillMaxSize(), contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 16.dp)) {
                item {
                    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        b.goal?.takeIf { it.isNotEmpty() }?.let {
                            Text(it, Modifier.clickable { fullGoal = !fullGoal }, style = Face.ui(14.sp), color = Palette.subtle, maxLines = if (fullGoal) Int.MAX_VALUE else 3, overflow = TextOverflow.Ellipsis)
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(22.dp)) {
                            for (lane in b.lanes) {
                                Column {
                                    Text("${lane.rows.size}", style = Face.display(40.sp, FontWeight.Black), color = laneTone(lane.id).color)
                                    Caps(lane.title)
                                }
                            }
                        }
                        val n = { id: String -> b.lanes.firstOrNull { it.id == id }?.rows?.size ?: 0 }
                        LaneBar(n("lane_done"), n("lane_review"), n("lane_doing"), b.lanes.sumOf { it.rows.size })
                    }
                }
                for (lane in b.lanes) {
                    val tone = laneTone(lane.id)
                    item(key = lane.id) {
                        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 22.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Text(lane.title.uppercase(), style = Face.display(22.sp), color = tone.color)
                                Text("${lane.rows.size}", Modifier.padding(bottom = 3.dp), style = Face.mono(12.sp, FontWeight.Bold), color = Palette.dim)
                            }
                            HorizontalDivider(color = tone.color.copy(alpha = 0.6f))
                        }
                    }
                    if (lane.rows.isEmpty()) {
                        item { Caps("Nothing here", Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) }
                    } else if (lane.id == "lane_done") {
                        // Done is the record, not the work: its latest rows, one line each.
                        val latest = lane.rows.sortedByDescending { it.updatedAt ?: 0 }
                        val shown = if (allDone) latest else latest.take(DONE_SHOWN)
                        items(shown, key = { "done-${it.id}" }) { r -> DoneRow(r, Modifier.clickable { open(r) }) }
                        if (lane.rows.size > DONE_SHOWN) {
                            item {
                                TextButton({ allDone = !allDone }, Modifier.padding(horizontal = 8.dp)) {
                                    Text(if (allDone) "Show the latest $DONE_SHOWN" else "Show all ${lane.rows.size}", style = Face.mono(12.sp, FontWeight.Bold), color = Palette.ok)
                                }
                            }
                        }
                    } else {
                        items(lane.rows, key = { "${lane.id}-${it.id}" }) { r ->
                            BoardRowCard(r, b.groupTitle(r.group), tone, live = lane.id == "lane_doing", Modifier.padding(horizontal = 16.dp, vertical = 5.dp).clickable { open(r) })
                        }
                    }
                }
            }
        }
    }
}

/** A done row on one line: a check, its number and its title. */
@Composable
private fun DoneRow(r: BoardDetail.Row, modifier: Modifier) {
    Column(modifier.padding(horizontal = 16.dp)) {
        Row(Modifier.padding(vertical = 7.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("✓", style = Face.ui(12.sp, FontWeight.Bold), color = Palette.ok)
            r.live?.number?.let { Text("#$it", style = Face.mono(11.sp, FontWeight.Bold), color = Palette.dim) }
            Text(r.live?.title ?: r.note ?: "A note", style = Face.ui(13.sp), color = Palette.subtle, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        HorizontalDivider(color = Palette.raised.copy(alpha = 0.6f))
    }
}

/** A board's row as a card in its lane's tone: the item's number and kind, its title, the row's note and its group. */
@Composable
private fun BoardRowCard(r: BoardDetail.Row, group: String?, tone: Tone, live: Boolean, modifier: Modifier) {
    ToneCard(tone, modifier, glow = live) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            val l = r.live
            if (l?.number != null) {
                Text("#${l.number}", style = Face.mono(12.sp, FontWeight.Bold), color = tone.color)
                l.kind?.let { Caps(if (it == "native") "issue" else it) }
            } else {
                Caps("note")
            }
            Spacer(Modifier.weight(1f))
            l?.state?.let { StateChip(it) }
        }
        r.live?.title?.let { Text(it, style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg, maxLines = 3, overflow = TextOverflow.Ellipsis) }
        val note = r.note
        if (!note.isNullOrEmpty() && !repeats(note, r.live?.title)) {
            Text(note, style = Face.ui(13.sp), color = Palette.subtle, maxLines = if (r.live == null) 5 else 2, overflow = TextOverflow.Ellipsis)
        } else if (r.live == null) {
            r.pointer?.url?.let { Text(it, style = Face.mono(11.sp), color = Palette.subtle, maxLines = 2) }
        }
        group?.let { Caps(it) }
    }
}

/** A note that only says the title again (rows brought in from GitHub were noted with their title). */
private fun repeats(note: String, title: String?) = !title.isNullOrEmpty() && note.lowercase().startsWith(title.lowercase())

/** A native issue, by its id. */
@Composable
fun IssueScreen(api: Api, id: String) {
    Column(Modifier.fillMaxSize()) {
        DetailBar("Issue")
        Loaded(key = id, load = { Issue.of(api.get("/api/issues/$id")) }) { issue, _ -> IssueBody(issue) }
    }
}

@Composable
private fun IssueBody(issue: Issue) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp).widthIn(max = 760.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("#${issue.number}", style = Face.mono(13.sp, FontWeight.Bold), color = Palette.accent)
            StateChip(issue.state)
            for (l in issue.labels) Caps(l)
        }
        Text(issue.title, style = Face.display(32.sp), color = Palette.fg)
        ChatText(issue.body)
    }
}

/** The current project's open issues, from its menu. */
@Composable
fun IssuesScreen(api: Api) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    Column(Modifier.fillMaxSize()) {
        DetailBar("Issues · ${scope.current?.name ?: ""}")
        val p = scope.current ?: return@Column Loading()
        Loaded(key = p.id, load = { api.get("/api/projects/${p.id}/issues").list("issues", Issue::of) }) { issues, _ ->
            if (issues.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Caps("No open issues") }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(issues, key = { it.id }) { i ->
                        Row(
                            Modifier.fillMaxWidth().clickable { nav.push(Dest.IssueRef(i.id)) }.padding(horizontal = 16.dp, vertical = 10.dp),
                            horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("#${i.number}", Modifier.widthIn(min = 44.dp), style = Face.mono(12.sp, FontWeight.Medium), color = Palette.dim)
                            Text(i.title, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.Medium), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            StateChip(i.state)
                        }
                        HorizontalDivider(color = Palette.raised)
                    }
                }
            }
        }
    }
}
