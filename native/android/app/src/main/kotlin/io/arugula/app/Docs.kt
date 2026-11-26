// Docs, the third place (Arugula #1362, #1364): the current project's Docs,
// as the web's Docs place lists them (web/src/platform/pages/docs.tsx) and
// the iPhone's DocViews.swift does: the latest changed first, in Today,
// Yesterday, This week and Earlier, each row its layout's mark, its title and
// when it was edited. A team's project has My Docs (what the reader and their
// agents made) and Team Docs, the one used last remembered; a personal one is
// My Docs alone.
//
// A Doc opens in a web view of control's `/embed/p/:project/docs/:doc`
// (web/src/shell/doc-embed.tsx), signed in with the app's session: its cookie
// stays a native session there, which reads the Doc and its thread and posts
// comments, and nothing more. Its prototypes run in the page's frames. The
// page tells the app about a link through `window.webkit.messageHandlers
// .arugula`, which iOS has; here a script at the document's start makes it,
// for control's own origin only, posting to a message listener. Another of
// the project's Docs opens in its place; another project's over everything;
// a board on the Overview; an issue over the list; anything else in the
// browser. On a wide tablet the Doc sits beside the list; on a phone it opens
// over it, with Back.

package io.arugula.app

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.text.format.DateUtils
import android.webkit.CookieManager
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Slideshow
import androidx.compose.material.icons.outlined.Tab
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/** A project Doc, as a link to its page names it; [label] is what the list or the link called it. */
data class DocRef(val project: String, val doc: String, val label: String? = null)

/** A project Doc on its project's list (the web's `ProjectDoc`), its times in milliseconds. */
data class ProjectDoc(val id: String, val project: String, val title: String, val author: String, val layout: String?, val updatedAt: Long) {
    val ref get() = DocRef(project, id, title)

    companion object {
        /** [author]: the account that made it, a machine's agent's owner's included; "" for one an agent session made. */
        fun of(o: JSONObject) = ProjectDoc(o.getString("id"), o.getString("project"), o.optString("title"), o.optString("author"), o.str("layout"), o.optLong("updated_at"))
    }
}

/** The web's groups (docs-list.tsx's `ageGroup`), by the day each Doc was last changed, the latest first. */
fun docGroups(docs: List<ProjectDoc>, now: Long, zone: ZoneId = ZoneId.systemDefault()): List<Pair<String, List<ProjectDoc>>> {
    val day = LocalDate.ofInstant(java.time.Instant.ofEpochMilli(now), zone)
    fun start(d: LocalDate) = d.atStartOfDay(zone).toInstant().toEpochMilli()
    val today = start(day)
    val yesterday = start(day.minusDays(1))
    val week = start(day.minusDays(6))
    val out = mutableListOf<Pair<String, MutableList<ProjectDoc>>>()
    for (d in docs.sortedByDescending { it.updatedAt }) {
        val title = when {
            d.updatedAt >= today -> "Today"
            d.updatedAt >= yesterday -> "Yesterday"
            d.updatedAt >= week -> "This week"
            else -> "Earlier"
        }
        if (out.lastOrNull()?.first == title) out.last().second.add(d) else out.add(title to mutableListOf(d))
    }
    return out
}

private enum class DocsTab(val label: String) { MINE("My Docs"), TEAM("Team Docs") }

/** Docs: the current project's Docs, newest first; one opens on the place's stack. */
@Composable
fun DocsScreen(api: Api, open: DocRef?) {
    val scope = LocalScope.current
    val nav = LocalNav.current
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences("arugula", Context.MODE_PRIVATE) }
    var tab by remember { mutableStateOf(if (prefs.getString("docs-tab", null) == DocsTab.TEAM.name) DocsTab.TEAM else DocsTab.MINE) }
    // "edited …" moves on with the clock, whether or not the list changed.
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            now = System.currentTimeMillis()
        }
    }
    Column(Modifier.fillMaxSize().testTag("docs-place")) {
        TabHeading(AppTab.DOCS.label)
        val p = scope.current ?: return@Column Loading()
        Loaded(key = p.id, every = 60_000, load = { api.get("/api/projects/${p.id}/docs").list("docs", ProjectDoc::of) }) { docs, _ ->
            val team = p.team != null
            val me = scope.me?.account
            val shown = if (team && tab == DocsTab.MINE) docs.filter { me != null && it.author == me } else docs
            Column(Modifier.fillMaxSize()) {
                if (team && docs.isNotEmpty()) {
                    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 8.dp).testTag("docs-tab")) {
                        DocsTab.entries.forEachIndexed { i, t ->
                            SegmentedButton(
                                selected = tab == t,
                                onClick = {
                                    tab = t
                                    prefs.edit().putString("docs-tab", t.name).apply()
                                },
                                shape = SegmentedButtonDefaults.itemShape(i, DocsTab.entries.size),
                                colors = SegmentedButtonDefaults.colors(
                                    activeContainerColor = Palette.accent, activeContentColor = Palette.void,
                                    inactiveContainerColor = Palette.void, inactiveContentColor = Palette.subtle,
                                    activeBorderColor = Palette.raised, inactiveBorderColor = Palette.raised,
                                ),
                                icon = {},
                            ) { Text(t.label, style = Face.ui(13.sp, FontWeight.Medium)) }
                        }
                    }
                }
                Box(Modifier.fillMaxSize()) {
                    // Always a list, so pulling down reads it again even when it's empty.
                    LazyColumn(Modifier.fillMaxSize().testTag("docs-list")) {
                        for ((title, group) in docGroups(shown, now)) {
                            item(key = "group-$title") { Caps(title, Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 4.dp)) }
                            items(group, key = { it.id }) { d ->
                                DocRow(
                                    d, now, picked = open?.doc == d.id,
                                    modifier = Modifier.clickable { nav.replace(Dest.Doc(d.ref)) }.testTag("doc-row-${d.id}"),
                                )
                                HorizontalDivider(color = Palette.raised)
                            }
                        }
                    }
                    if (docs.isEmpty()) {
                        Column(
                            Modifier.align(Alignment.Center).padding(24.dp).testTag("docs-none"),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Caps("No Docs yet")
                            Text(
                                "Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here.",
                                Modifier.width(320.dp), style = Face.ui(14.sp), color = Palette.subtle, textAlign = TextAlign.Center,
                            )
                        }
                    } else if (shown.isEmpty()) {
                        Caps(
                            "None of ${p.name}'s Docs is yours or your agents' yet",
                            Modifier.align(Alignment.Center).padding(24.dp).testTag("docs-none"),
                        )
                    }
                }
            }
        }
    }
}

/** A Doc on the list, as the web's row: its layout's mark (a tabs or a prototype Doc; a plain one has none), its title, and when it was edited. */
@Composable
private fun DocRow(doc: ProjectDoc, now: Long, picked: Boolean, modifier: Modifier) {
    Row(
        modifier.fillMaxWidth().background(if (picked) Palette.surface else Palette.void).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        when (doc.layout) {
            "tabs" -> Icon(Icons.Outlined.Tab, "Tabs Doc", Modifier.size(16.dp), tint = Palette.accent)
            "prototype" -> Icon(Icons.Outlined.Slideshow, "Prototype Doc", Modifier.size(16.dp), tint = Palette.accent)
        }
        Text(doc.title.ifEmpty { "Untitled" }, Modifier.weight(1f), style = Face.ui(15.sp, FontWeight.Medium), color = Palette.fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.width(8.dp))
        Text(
            "edited ${DateUtils.getRelativeTimeSpanString(doc.updatedAt, now, DateUtils.MINUTE_IN_MILLIS, DateUtils.FORMAT_ABBREV_RELATIVE)}",
            style = Face.mono(11.sp), color = Palette.dim, maxLines = 1,
        )
    }
}

/** The app's sign-in as the web view needs it: control's address and the session's cookie (`name=value`), from what the app keeps. */
class WebSession(val url: Uri, val cookie: String) {
    /** Control's origin, as a message listener's rule names it. */
    val origin: String get() = "${url.scheme}://${url.encodedAuthority}"

    fun page(d: DocRef): String = "${url.toString().trimEnd('/')}/embed/p/${d.project}/docs/${d.doc}"

    companion object {
        fun of(stored: String?): WebSession? = runCatching {
            val v = JSONObject(stored!!)
            val url = Uri.parse(v.getString("url").trimEnd('/'))
            val cookie = v.getString("cookie")
            if (url.host.isNullOrEmpty() || !cookie.contains('=')) null else WebSession(url, cookie)
        }.getOrNull()
    }
}

/** A Doc, its page in a web view, with its title and Back. */
@Composable
fun DocScreen(api: Api, ref: DocRef) {
    val nav = LocalNav.current
    val scope = LocalScope.current
    val context = LocalContext.current
    val coroutines = rememberCoroutineScope()
    var title by remember(ref) { mutableStateOf<String?>(null) }
    var problem by remember(ref) { mutableStateOf<String?>(null) }
    var loading by remember(ref) { mutableStateOf(true) }
    var ready by remember(ref) { mutableStateOf(false) }
    // A new web view when the last one's page process went (memory).
    var generation by remember(ref) { mutableIntStateOf(0) }
    val session = remember(api) { WebSession.of(api.stored) }

    /** A link in the Doc. */
    fun follow(i: JSONObject) {
        val project = i.str("project")
        val id = i.str("id")
        val url = i.str("url")
        when (i.optString("kind")) {
            "doc" -> if (project != null && id != null) nav.replace(Dest.Doc(DocRef(project, id), over = project != scope.current?.id))
            "board" -> if (project != null && id != null) coroutines.launch {
                // The Overview opens a board's thread, as from its list.
                runCatching { BoardDetail.of(api.get("/api/boards/$id")) }
                    .onSuccess { b -> nav.open(AppTab.OVERVIEW, Dest.BoardThread(Board(id, b.title, b.goal, null, null, b.lead))) }
                    .onFailure { url?.let { browse(context, it) } }
            }
            "issue" -> if (id != null) nav.push(Dest.IssueRef(id, project))
            else -> url?.let { browse(context, it) }
        }
    }

    Column(Modifier.fillMaxSize().testTag("doc-${ref.doc}")) {
        DetailBar(title ?: ref.label?.ifEmpty { null } ?: "Doc")
        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (session == null) {
                Problem("This app isn't signed in.")
            } else {
                key(ref.doc, generation) {
                    DocWebView(
                        session.page(ref), session,
                        intent = { i ->
                            when (i.optString("kind")) {
                                "ready" -> {
                                    title = i.str("title")?.ifEmpty { null } ?: title
                                    loading = false
                                    ready = true
                                }
                                "signed_out" -> problem = "Control didn't take this app's sign-in for the Doc. Sign out and in again from your picture."
                                else -> follow(i)
                            }
                        },
                        finished = { t -> if (title == null) title = t?.ifEmpty { null } },
                        failed = { e ->
                            loading = false
                            problem = e
                        },
                        gone = {
                            loading = true
                            generation++
                        },
                        outside = { browse(context, it) },
                    )
                }
                if (loading && problem == null) Loading()
                problem?.let { Problem(it) }
                // For the flow test: the page said it drew the Doc.
                if (ready) Box(Modifier.size(1.dp).testTag("doc-ready"))
            }
        }
    }
}

/**
 * The embed page in a WebView, its cookie the app's session. The Doc's own
 * page loads here, and the frames its prototypes run in; any other page the
 * view is sent to goes to [outside].
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun DocWebView(
    page: String, session: WebSession, intent: (JSONObject) -> Unit, finished: (String?) -> Unit,
    failed: (String) -> Unit, gone: () -> Unit, outside: (String) -> Unit,
) {
    val onIntent by rememberUpdatedState(intent)
    val onFinished by rememberUpdatedState(finished)
    val onFailed by rememberUpdatedState(failed)
    val onGone by rememberUpdatedState(gone)
    val onOutside by rememberUpdatedState(outside)
    val home = Uri.parse(page)
    AndroidView(
        factory = { ctx ->
            WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)
            WebView(ctx).apply {
                setBackgroundColor(Palette.void.toArgb())
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                // The page's posts, from control's origin and its top frame
                // only: a prototype's frame is another origin, and says nothing.
                val origins = setOf(session.origin)
                if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
                    WebViewCompat.addWebMessageListener(this, "arugulaAndroid", origins) { _, message, _, main, _ ->
                        if (main) message.data?.let { runCatching { JSONObject(it) }.getOrNull() }?.let { onIntent(it) }
                    }
                }
                val start = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
                if (start) WebViewCompat.addDocumentStartJavaScript(this, SHIM, origins)
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        val to = request.url
                        if (!request.isForMainFrame || to.scheme in listOf("about", "blob", "data")) return false
                        if (to.host == home.host && to.port == home.port && to.path == home.path) return false
                        onOutside(to.toString())
                        return true
                    }

                    override fun onPageStarted(view: WebView, url: String?, favicon: android.graphics.Bitmap?) {
                        // A WebView too old for a script at the document's start: as early as it can.
                        if (!start) view.evaluateJavascript(SHIM, null)
                    }

                    override fun onPageFinished(view: WebView, url: String?) = onFinished(view.title)

                    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                        if (request.isForMainFrame) onFailed(error.description?.toString() ?: "The Doc didn't load.")
                    }

                    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                        onGone()
                        return true
                    }
                }
                val cookies = CookieManager.getInstance()
                cookies.setAcceptCookie(true)
                cookies.setAcceptThirdPartyCookies(this, false)
                val secure = if (session.url.scheme == "https") "; Secure" else ""
                cookies.setCookie(session.url.toString(), "${session.cookie}; Path=/$secure") { loadUrl(page) }
            }
        },
        onRelease = { view ->
            view.stopLoading()
            view.destroy()
        },
        modifier = Modifier.fillMaxSize().testTag("doc-web"),
    )
}

/**
 * What iOS's WKWebView gives the page: `window.webkit.messageHandlers.arugula`,
 * which doc-embed.tsx posts its intents to, here onto the message listener.
 */
private const val SHIM = """
(function () {
  if (!window.arugulaAndroid) return;
  var w = window.webkit = window.webkit || {};
  var h = w.messageHandlers = w.messageHandlers || {};
  h.arugula = { postMessage: function (m) { window.arugulaAndroid.postMessage(JSON.stringify(m)); } };
})();
"""

/**
 * The session's cookie out of the WebView's store, as the sign-in ends: it's
 * the app's, put there for each Doc it opens, and outlives no session.
 */
fun forgetWebCookies() {
    runCatching { CookieManager.getInstance().apply { removeAllCookies(null); flush() } }
}

/** A link the app has no place for: the browser, or the app that takes it. */
fun browse(context: Context, link: String) {
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(link)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (_: ActivityNotFoundException) {
        openTab(context, link)
    }
}
