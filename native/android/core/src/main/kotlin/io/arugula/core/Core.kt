package io.arugula.core

import android.content.Context
import org.json.JSONObject

/**
 * The Rust core (native/core) as Kotlin calls it: signing in, control's
 * requests with the session, a machine's channel and chat's live stream.
 * The same core the iPhone app runs on, with the same JSON.
 *
 * Calls that reach the network block: make them off the main thread.
 */
object Core {
    /** Once, before anything else: TLS checks control's certificate against Android's trust store through it. */
    fun init(context: Context) = Native.init(context.applicationContext)

    /** Whether control at [url] still serves this build: throws [CoreError] with `updateApp` when it doesn't. Blocks. */
    fun checkApp(url: String) = Native.checkApp(url)

    /**
     * Whether a pane shows in the project [atId] ([atPersonal]: the reader's
     * personal project; null: every pane shows), the web's `inShown`
     * (Arugula #1355): its session's project is it, or the card it works on
     * is, or it's placed nowhere on a machine that's [yours], in your
     * personal project. A session's own: [onIssueProject] null.
     */
    fun shown(atId: String?, atPersonal: Boolean, sessionProject: String?, onIssueProject: String?, yours: Boolean): Boolean =
        Native.shown(atId, atPersonal, sessionProject, onIssueProject, yours)

    /**
     * The machines as Machines lists them, as `aru_machines` (native/core's
     * ffi.rs): [input] is `{at, recent, hosts: [{id, name, online, owner,
     * hosted, panes}], pins}`; the answer `{order, machines, quiet, pinned}`.
     */
    fun machines(input: JSONObject): JSONObject = JSONObject(Native.machines(input.toString()))

    /**
     * Where you were in [project] (Arugula #1360), from its `app.pane.<project>`
     * pref, as the web reads it: `{machine, pane}`, or null when [path] names
     * no pane the app can place.
     */
    fun keptPane(path: String, project: String): JSONObject? = Native.keptPane(path, project)?.let(::JSONObject)

    /** The `app.pane.<project>` pref's value for [pane] on [machine], as the web writes it. */
    fun panePath(project: String, machine: String, pane: Int): String = Native.panePath(project, machine, pane).orEmpty()

    /** Where control's page sends a sign-in's grant: `arugula://signin?ticket=…&grant=…`. */
    const val CALLBACK_SCHEME = "arugula"
}

/**
 * What the core refused, as Swift's `CoreError`: control's status and words,
 * and a `reason` an app acts on: `signed_out` (sign in again),
 * `update_app` (control no longer serves this build), `older_control`
 * (control doesn't take the app's sign-in), or a machine's
 * code (`machine-token`, `malformed`).
 */
class CoreError(val json: String) : Exception(errorOf(json)) {
    private val v = runCatching { JSONObject(json) }.getOrNull()
    val status: Int? = v?.optInt("status", -1)?.takeIf { it >= 0 }
    val reason: String? = v?.optString("reason")?.takeIf { it.isNotEmpty() && it != "null" }
    val signedOut get() = reason == "signed_out"

    /** Control no longer serves this build: say to update the app. */
    val updateApp get() = reason == "update_app"

    private companion object {
        fun errorOf(json: String) = runCatching { JSONObject(json).getString("error") }.getOrDefault(json)
    }
}

/** Called from the core's threads when there's something to take: hop to your own before touching anything. */
fun interface Notify {
    fun ready()
}

/**
 * A sign-in, the CLI's way (#999): open [link] in a Custom Tab, where someone
 * signed in clicks Allow; the page is sent to `arugula://signin` with a
 * one-time grant, and [finish] swaps it, with a verifier only this holds, for
 * a session.
 */
class SignIn private constructor(private var h: Long) : AutoCloseable {
    private val info = JSONObject(Native.signInInfo(h))

    /** Control's Allow page, for the Custom Tab. */
    val link: String = info.getString("link")
    val ticket: String = info.getString("ticket")
    val expiresInSecs: Long = info.getLong("expires_in_secs")

    /** Swap the grant in [callback], the address the page was sent to, for a session: JSON to keep, for [Control]. Blocks. */
    fun finish(callback: String): String = Native.signInFinish(h, callback)

    override fun close() {
        if (h != 0L) Native.signInFree(h).also { h = 0 }
    }

    companion object {
        /** Ask control at [url] for a sign-in; [name] is what the Allow page calls this. Blocks. */
        fun start(url: String, name: String) = SignIn(Native.signInStart(url, name))
    }
}

/** Requests as the account, from the session [SignIn.finish] gave. */
class Control(stored: String) : AutoCloseable {
    internal var h = Native.controlOpen(stored)
        private set

    /** A request: its JSON. [body] is JSON or null. Throws [CoreError]; `signedOut` means sign in again. */
    fun call(method: String, path: String, body: String? = null): String = Native.controlCall(h, method, path, body)

    fun get(path: String) = call("GET", path)

    fun post(path: String, body: String) = call("POST", path, body)

    /** End the session at control. Forget it either way. */
    fun signOut() = Native.controlSignOut(h)

    override fun close() {
        if (h != 0L) Native.controlFree(h).also { h = 0 }
    }
}

/**
 * A channel to one machine through control's relay, opened with a machine
 * token. [notify] says there's something to [feed].
 */
class Link(control: Control, machine: String, notify: Notify) : AutoCloseable {
    private var h = Native.linkOpen(control.h, machine, notify)

    /**
     * Take what's queued: `terms[i]` (or null) takes `panes[i]`'s bytes.
     * Returns bits: [TERM], [PANES], [CLOSED], [SAID].
     */
    fun feed(panes: IntArray = IntArray(0), terms: Array<Term?> = emptyArray()): Int =
        Native.linkFeed(h, panes, LongArray(terms.size) { terms[it]?.h ?: 0L })

    /** The machine's panes, tabs and sessions as JSON (`aru_link_panes`), or null before its first word. */
    val panes: String? get() = Native.linkPanes(h)

    /** Why the channel closed, once it has. */
    val closed: String? get() = Native.linkClosed(h)

    /** What the machine last refused, in its words, once. */
    val said: String? get() = Native.linkSaid(h)

    /** Receive [pane], sized to the view: its snapshot, then its output. */
    fun attach(tab: Int, pane: Int, cols: Int, rows: Int) = Native.linkAttach(h, tab, pane, cols, rows)

    fun view(tab: Int, pane: Int, cols: Int, rows: Int) = Native.linkView(h, tab, pane, cols, rows)

    /** Receive a whole tab's [panes] at the size it already is. */
    fun watch(tab: Int, panes: IntArray, cols: Int, rows: Int) = Native.linkWatch(h, tab, panes, cols, rows)

    fun detach(panes: IntArray) = Native.linkDetach(h, panes)

    /** Type into [pane]. */
    fun input(pane: Int, data: ByteArray) = Native.linkInput(h, pane, data)

    /** One request to the machine's own API: `{"status": N, "body": ...}`. Blocks. */
    fun request(method: String, path: String, body: String? = null): String = Native.linkRequest(h, method, path, body)

    override fun close() {
        if (h != 0L) Native.linkFree(h).also { h = 0 }
    }

    companion object {
        const val TERM = 1
        const val PANES = 2
        const val CLOSED = 4
        const val SAID = 8
    }
}

/** Chat's live stream: [notify] says an event is queued for [next]. */
class ChatLive(control: Control, notify: Notify) : AutoCloseable {
    private var h = Native.chatOpen(control.h, notify)

    /** Control's JSON (`{"type": "message", ...}`), `{"type":"closed","why":...}` once it has ended, or null. */
    fun next(): String? = Native.chatNext(h)

    override fun close() {
        if (h != 0L) Native.chatFree(h).also { h = 0 }
    }
}
