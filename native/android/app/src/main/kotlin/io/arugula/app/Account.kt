// The account, in every place's heading (Arugula #1362, the iPhone's #1361),
// now that You has left the tab bar: your picture opens your name, Settings,
// Send feedback, the app's version and Sign out, as the web's avatar menu;
// the bell beside it opens the Inbox. Settings is the web's page: control
// doesn't take this app's session on it, so it opens in a Custom Tab, at the
// platform control's `/control.json` names. Send feedback goes to control's
// `POST /api/feedback` as the person signed in; while control refuses the app
// there (`page_only`), the sheet says so and offers the web's page instead.
// The iPhone's Account.swift.

package io.arugula.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Feedback
import androidx.compose.material.icons.automirrored.outlined.Logout
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.arugula.core.CoreError
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** What every place's heading does with its buttons: New, the bell, and the account's. Unset, the heading has the project's menu alone. */
class Heading(val api: Api, val model: AppModel, val makeNew: () -> Unit, val showInbox: () -> Unit)

val LocalHeading = staticCompositionLocalOf<Heading?> { null }

/** The app's version and build, as Settings › Apps says it. */
val appVersion get() = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})"

/** Where the platform's page is, as `/control.json` names it: the web's Settings, and anything else the app can't open itself. */
suspend fun platformOrigin(control: String): String? = withContext(Dispatchers.IO) {
    runCatching {
        JSONObject(URL(control.trimEnd('/') + "/control.json").readText()).str("platform")?.trimEnd('/')?.takeIf { it.isNotEmpty() }
    }.getOrNull()
}

/** One of the heading's round buttons: 36dp, with its word for TalkBack. */
@Composable
fun HeadingButton(icon: ImageVector, label: String, modifier: Modifier = Modifier, primary: Boolean = false, onClick: () -> Unit) {
    Box(
        modifier.size(36.dp).clip(CircleShape).background(if (primary) Palette.accent else Palette.raised).clickable(onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, Modifier.size(20.dp), tint = if (primary) Palette.void else Palette.fg)
    }
}

/** Your picture: your initials in your tone (as chat shows people), opening the account's menu. */
@Composable
fun AccountMenu(heading: Heading) {
    val scope = LocalScope.current
    val context = LocalContext.current
    val coroutines = rememberCoroutineScope()
    var open by remember { mutableStateOf(false) }
    var feedback by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    val me = scope.me
    Box {
        Box(Modifier.size(36.dp).clip(CircleShape).clickable { open = true }.testTag("account").semantics { contentDescription = "You: ${me?.name ?: "your account"}" }) {
            Avatar(me?.name?.ifEmpty { null } ?: me?.login ?: "?", 36)
        }
        DropdownMenu(open, { open = false }, containerColor = Palette.surface) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 6.dp)) {
                Text(me?.name?.ifEmpty { null } ?: "Signed in", style = Face.ui(15.sp, FontWeight.SemiBold), color = Palette.fg)
                me?.login?.let { Text(it, style = Face.mono(12.sp), color = Palette.dim) }
            }
            HorizontalDivider(color = Palette.raised)
            DropdownMenuItem(
                text = { Text("Settings", style = Face.ui(15.sp)) },
                leadingIcon = { Icon(Icons.Outlined.Settings, null, tint = Palette.subtle) },
                onClick = {
                    open = false
                    coroutines.launch {
                        val p = platformOrigin(heading.model.url)
                        if (p == null) problem = "Settings didn't open: control didn't say where its page is." else openTab(context, "$p/settings")
                    }
                },
                modifier = Modifier.testTag("account-settings"),
            )
            DropdownMenuItem(
                text = { Text("Send feedback", style = Face.ui(15.sp)) },
                leadingIcon = { Icon(Icons.Outlined.Feedback, null, tint = Palette.subtle) },
                onClick = {
                    open = false
                    feedback = true
                },
                modifier = Modifier.testTag("account-feedback"),
            )
            HorizontalDivider(color = Palette.raised)
            Caps("Arugula for Android $appVersion", Modifier.padding(horizontal = 12.dp, vertical = 6.dp))
            DropdownMenuItem(
                text = { Text("Sign out", style = Face.ui(15.sp), color = Palette.danger) },
                leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Logout, null, tint = Palette.danger) },
                onClick = {
                    open = false
                    heading.model.signOut()
                },
                modifier = Modifier.testTag("sign-out"),
            )
        }
    }
    problem?.let {
        androidx.compose.material3.AlertDialog(
            { problem = null },
            confirmButton = { TextLink("OK") { problem = null } },
            text = { Text(it, style = Face.ui(14.sp)) },
            containerColor = Palette.bg,
        )
    }
    if (feedback) FeedbackSheet(heading) { feedback = false }
}

/** Send feedback, as the web's sheet: what it is, a title and what happened. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FeedbackSheet(heading: Heading, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val coroutines = rememberCoroutineScope()
    var kind by remember { mutableStateOf("bug") }
    var title by remember { mutableStateOf("") }
    var words by remember { mutableStateOf("") }
    var contact by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    /** Control won't take it from the app: send it from the web's page. */
    var pageOnly by remember { mutableStateOf(false) }
    var sent by remember { mutableStateOf<String?>(null) }
    val kinds = listOf("bug" to "Something's broken", "missing" to "Something's missing", "idea" to "An idea", "praise" to "Praise", "other" to "Other")

    fun send() = coroutines.launch {
        busy = true
        val body = JSONObject().put("kind", kind).put("title", title.trim().take(200)).put("body", words).put("contact_ok", contact)
            .put("source", "person").put("surface", "other").put("context", JSONObject().put("version", appVersion).put("os", "other"))
        try {
            sent = heading.api.post("/api/feedback", body).str("shown")?.takeIf { it.isNotEmpty() } ?: "feedback"
            problem = null
        } catch (e: CoreError) {
            if (e.reason == "page_only") {
                pageOnly = true
                problem = "Control doesn't take feedback from the app yet."
            } else {
                problem = describe(e)
            }
        } catch (e: Exception) {
            problem = describe(e)
        }
        busy = false
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Send feedback", Modifier.weight(1f), style = Face.display(26.sp), color = Palette.fg)
                TextLink(if (sent == null) "Cancel" else "Done", onDismiss)
            }
            Caps("What is it")
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for ((k, says) in kinds) {
                    FilterChip(
                        kind == k, { kind = k }, { Text(says, style = Face.ui(13.sp)) },
                        colors = FilterChipDefaults.filterChipColors(selectedContainerColor = Palette.accent, selectedLabelColor = Palette.void, labelColor = Palette.subtle),
                    )
                }
            }
            OutlinedTextField(title, { title = it }, Modifier.fillMaxWidth().testTag("feedback-title"), singleLine = true, label = { Text("Title") }, textStyle = Face.ui(16.sp))
            OutlinedTextField(
                words, { words = it }, Modifier.fillMaxWidth().heightIn(min = 120.dp).testTag("feedback-body"),
                label = { Text("What happened") }, textStyle = Face.ui(16.sp), minLines = 4, maxLines = 12,
            )
            Row(Modifier.clickable { contact = !contact }, verticalAlignment = Alignment.CenterVertically) {
                Checkbox(contact, { contact = it }, colors = CheckboxDefaults.colors(checkedColor = Palette.accent, checkmarkColor = Palette.void))
                Text("You may contact me about it", style = Face.ui(14.sp), color = Palette.fg)
            }
            val s = sent
            if (s != null) {
                Text("Sent as $s. Thank you.", style = Face.ui(14.sp), color = Palette.ok)
            } else {
                problem?.let { Text(it, style = Face.mono(12.sp), color = Palette.danger) }
                if (pageOnly) {
                    AnswerButton("Send it from Arugula's page") {
                        coroutines.launch { platformOrigin(heading.model.url)?.let { openTab(context, it) } }
                    }
                }
                AnswerButton(if (busy) "Sending…" else "Send", Modifier.testTag("feedback-send"), primary = true, enabled = !busy && title.isNotBlank()) { send() }
            }
        }
    }
}

/** One entry of the Inbox, as `/api/inbox` lists it. */
data class InboxEntry(
    val item: String, val reason: String, val repo: String, val number: Int, val state: String, val title: String,
    val url: String, val updatedAt: Long, val seenAt: Long?,
) {
    companion object {
        fun of(o: JSONObject) = InboxEntry(
            o.getString("item"), o.optString("reason"), o.optString("repo"), o.optInt("number"), o.optString("state"),
            o.optString("title"), o.optString("url"), o.optLong("updated_at"), o.long("seen_at"),
        )
    }
}

/**
 * What's waiting for you, as the web's Inbox lists it: PRs, issues and runs
 * in the current project, newest first; one opens on its page. Its count on
 * the bell, and a push landing here, are Arugula #1365.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InboxSheet(heading: Heading, onDismiss: () -> Unit) {
    val scope = LocalScope.current
    val context = LocalContext.current
    val coroutines = rememberCoroutineScope()
    var entries by remember { mutableStateOf<List<InboxEntry>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    val project = scope.current?.id
    androidx.compose.runtime.LaunchedEffect(project) {
        try {
            val path = project?.let { "/api/inbox?project=$it" } ?: "/api/inbox"
            entries = heading.api.get(path).list("entries", InboxEntry::of).sortedByDescending { it.updatedAt }
            problem = null
        } catch (e: Exception) {
            problem = describe(e)
        }
    }

    /** GitHub's items by their address; a native issue's or a run's is a path on the platform page. */
    fun open(e: InboxEntry) = coroutines.launch {
        if (Regex("^[a-z][a-z0-9+.-]*:", RegexOption.IGNORE_CASE).containsMatchIn(e.url)) {
            openTab(context, e.url)
        } else {
            platformOrigin(heading.model.url)?.let { openTab(context, it + "/" + e.url.trimStart('/')) }
        }
    }

    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = Palette.bg) {
        Column(Modifier.padding(bottom = 24.dp).testTag("inbox")) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Inbox", Modifier.weight(1f), style = Face.display(26.sp), color = Palette.fg)
                TextLink("Done", onDismiss)
            }
            val list = entries
            when {
                problem != null -> Text(problem!!, Modifier.padding(16.dp), style = Face.mono(12.sp), color = Palette.danger)
                list == null -> Caps("Looking…", Modifier.padding(16.dp))
                list.isEmpty() -> Caps("Nothing waiting for you", Modifier.padding(16.dp))
                else -> LazyColumn {
                    items(list, key = { "${it.item}:${it.reason}" }) { e ->
                        InboxRow(e, Modifier.clickable { open(e) })
                        HorizontalDivider(color = Palette.raised)
                    }
                }
            }
        }
    }
}

@Composable
private fun InboxRow(e: InboxEntry, modifier: Modifier) {
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            Box(Modifier.size(7.dp).clip(CircleShape).background(if (e.seenAt == null) Palette.accent else Palette.void))
            Text(if (e.number > 0) "${e.repo} #${e.number}" else e.repo, Modifier.weight(1f), style = Face.mono(11.sp), color = Palette.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
            StateChip(e.state)
        }
        Text(e.title, style = Face.ui(15.sp, if (e.seenAt == null) FontWeight.SemiBold else FontWeight.Normal), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Caps(e.reason.replace('_', ' '))
    }
}
