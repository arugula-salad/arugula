// The account, in every place's heading on a phone and at the rail's foot on
// a tablet (Arugula #1362, #1366), now that You has left the tab bar: your
// picture opens your name, Settings, Send feedback, the app's version and
// Sign out, as the web's avatar menu; the bell beside it opens the Inbox
// (Inbox.kt, Arugula #1365). Settings is the web's page, in a web view here:
// control hands the app's sign-in to it (`POST /api/handoff`, Arugula #1420)
// for a web session of its own, which leaving it ends. Send feedback goes to
// control's `POST /api/feedback` as the person signed in. Your picture is
// control's (`/api/chat/avatar/{account}`), as the web shows it.
// The iPhone's Account.swift.

package io.arugula.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import android.annotation.SuppressLint
import android.graphics.BitmapFactory
import android.net.Uri
import android.webkit.CookieManager
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** What every place's heading does with its buttons: New, the bell, and the account's. Unset, the heading has the project's menu alone. */
class Heading(val api: Api, val model: AppModel, val makeNew: () -> Unit, val showInbox: () -> Unit) {
    /** The Inbox and the bell's count, the heading's and the rail's alike (Arugula #1365). */
    val inbox = InboxModel(api)
}

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

/** Your picture (your initials in your tone until it comes, as chat shows people), opening the account's menu. [tag]: `account` in a heading, `rail-account` on the rail. */
@Composable
fun AccountMenu(heading: Heading, tag: String = "account") {
    val scope = LocalScope.current
    val nav = LocalNav.current
    var open by remember { mutableStateOf(false) }
    var feedback by remember { mutableStateOf(false) }
    val me = scope.me
    val picture by produceState<ImageBitmap?>(null, me?.account) {
        val account = me?.account ?: return@produceState
        value = withContext(Dispatchers.IO) {
            runCatching { heading.api.control.bytes("/api/chat/avatar/$account") }.getOrNull()
                ?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }
        }
    }
    Box {
        Box(Modifier.size(36.dp).clip(CircleShape).clickable { open = true }.testTag(tag).semantics { contentDescription = "You: ${me?.name ?: "your account"}" }) {
            Avatar(me?.name?.ifEmpty { null } ?: me?.login ?: "?", 36, picture = picture)
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
                    nav.push(Dest.Settings)
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
    if (feedback) FeedbackSheet(heading) { feedback = false }
}

/** Send feedback, as the web's sheet: what it is, a title and what happened. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FeedbackSheet(heading: Heading, onDismiss: () -> Unit) {
    val coroutines = rememberCoroutineScope()
    var kind by remember { mutableStateOf("bug") }
    var title by remember { mutableStateOf("") }
    var words by remember { mutableStateOf("") }
    var contact by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var sent by remember { mutableStateOf<String?>(null) }
    val kinds = listOf("bug" to "Something's broken", "missing" to "Something's missing", "idea" to "An idea", "praise" to "Praise", "other" to "Other")

    fun send() = coroutines.launch {
        busy = true
        val body = JSONObject().put("kind", kind).put("title", title.trim().take(200)).put("body", words).put("contact_ok", contact)
            .put("source", "person").put("surface", "other").put("context", JSONObject().put("version", appVersion).put("os", "other"))
        try {
            sent = heading.api.post("/api/feedback", body).str("shown")?.takeIf { it.isNotEmpty() } ?: "feedback"
            problem = null
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
                AnswerButton(if (busy) "Sending…" else "Send", Modifier.testTag("feedback-send"), primary = true, enabled = !busy && title.isNotBlank()) { send() }
            }
        }
    }
}


/** Where Settings starts: the app's sign-in as kept, the hand-over's code and where the page may go in the web view. */
private class SettingsStart(val session: WebSession, val stored: String, val code: String, val hosts: Set<String>)

/**
 * Settings, the web's page, in a web view (Arugula #1420): the app asks
 * control for a one-time code with its session, and the web view posts it,
 * in the request's body, for a web session of its own and the page. Pages
 * off control's and the platform's hosts open in the browser. Leaving it
 * ends that web session.
 */
@Composable
fun SettingsScreen(api: Api) {
    val context = LocalContext.current
    var start by remember { mutableStateOf<SettingsStart?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    val session = remember(api) { WebSession.of(api.stored) }
    LaunchedEffect(api) {
        val s = session
        val stored = api.stored
        if (s == null || stored == null) return@LaunchedEffect run { problem = "This app isn't signed in." }
        try {
            val platform = platformOrigin(s.url.toString())?.let { Uri.parse(it).host }
            val code = api.post("/api/handoff").str("code") ?: return@LaunchedEffect run { problem = "Control didn't hand the sign-in over." }
            start = SettingsStart(s, stored, code, setOfNotNull(s.url.host, platform))
        } catch (e: Exception) {
            loading = false
            problem = describe(e)
        }
    }
    Column(Modifier.fillMaxSize().testTag("settings")) {
        DetailBar("Settings")
        Box(Modifier.weight(1f).fillMaxWidth()) {
            start?.let { st ->
                SettingsWebView(
                    st,
                    finished = { loading = false },
                    failed = { e ->
                        loading = false
                        problem = e
                    },
                    outside = { browse(context, it) },
                )
            }
            if (loading && problem == null) Loading()
            problem?.let { Problem(it) }
        }
    }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun SettingsWebView(start: SettingsStart, finished: () -> Unit, failed: (String) -> Unit, outside: (String) -> Unit) {
    val onFinished by rememberUpdatedState(finished)
    val onFailed by rememberUpdatedState(failed)
    val onOutside by rememberUpdatedState(outside)
    val base = start.session.url.toString().trimEnd('/')
    AndroidView(
        factory = { ctx ->
            WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)
            WebView(ctx).apply {
                setBackgroundColor(Palette.void.toArgb())
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        val to = request.url
                        if (!request.isForMainFrame || to.scheme in listOf("about", "blob", "data")) return false
                        if (to.host in start.hosts) return false
                        onOutside(to.toString())
                        return true
                    }

                    override fun onPageFinished(view: WebView, url: String?) = onFinished()

                    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                        if (request.isForMainFrame) onFailed(error.description?.toString() ?: "Settings didn't load.")
                    }
                }
                val cookies = CookieManager.getInstance()
                cookies.setAcceptCookie(true)
                cookies.setAcceptThirdPartyCookies(this, false)
                // The code is hex: nothing in it to escape.
                postUrl("$base/auth/handoff", "code=${start.code}&to=%2Fsettings".toByteArray())
            }
        },
        onRelease = { view ->
            view.stopLoading()
            view.destroy()
            endWebSession(base, start)
        },
        modifier = Modifier.fillMaxSize().testTag("settings-web"),
    )
}

/** Settings is left: the web view's own session ends at control, as signing out there would, and its cookie goes. */
private fun endWebSession(base: String, start: SettingsStart) {
    val session = start.session
    val cookies = CookieManager.getInstance()
    val name = session.cookie.substringBefore('=')
    val web = cookies.getCookie(base)?.split(';')?.map { it.trim() }?.firstOrNull { it.startsWith("$name=") } ?: return
    if (web == session.cookie) return
    val secure = if (session.url.scheme == "https") "; Secure" else ""
    cookies.setCookie(base, "$name=; Path=/; Max-Age=0$secure")
    cookies.flush()
    val stored = runCatching { JSONObject(start.stored).put("cookie", web) }.getOrNull() ?: return
    kotlinx.coroutines.CoroutineScope(Dispatchers.IO).launch {
        runCatching { io.arugula.core.Control(stored.toString()).use { it.signOut() } }
    }
}
