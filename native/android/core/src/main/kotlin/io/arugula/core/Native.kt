package io.arugula.core

import android.content.Context

/**
 * The core's JNI functions (native/core/src/kotlin.rs), one for each `aru_*`
 * function Swift calls. Handles are pointers as a Long. Use the classes in
 * Core.kt and Term.kt instead.
 */
internal object Native {
    init {
        System.loadLibrary("arugula_native_core")
    }

    @JvmStatic external fun init(context: Context)

    @JvmStatic external fun checkApp(url: String)

    @JvmStatic external fun signInStart(url: String, name: String): Long
    @JvmStatic external fun signInInfo(h: Long): String
    @JvmStatic external fun signInFinish(h: Long, callback: String): String
    @JvmStatic external fun signInFree(h: Long)

    @JvmStatic external fun controlOpen(stored: String): Long
    @JvmStatic external fun controlSignOut(h: Long)
    @JvmStatic external fun controlCall(h: Long, method: String, path: String, body: String?): String
    @JvmStatic external fun controlBytes(h: Long, path: String): String
    @JvmStatic external fun controlFree(h: Long)

    @JvmStatic external fun linkOpen(control: Long, machine: String, notify: Notify): Long
    @JvmStatic external fun linkFeed(h: Long, panes: IntArray, terms: LongArray): Int
    @JvmStatic external fun linkPanes(h: Long): String?
    @JvmStatic external fun linkClosed(h: Long): String?
    @JvmStatic external fun linkSaid(h: Long): String?
    @JvmStatic external fun linkWatch(h: Long, tab: Int, panes: IntArray, cols: Int, rows: Int)
    @JvmStatic external fun linkDetach(h: Long, panes: IntArray)
    @JvmStatic external fun linkAttach(h: Long, tab: Int, pane: Int, cols: Int, rows: Int)
    @JvmStatic external fun linkView(h: Long, tab: Int, pane: Int, cols: Int, rows: Int)
    @JvmStatic external fun linkInput(h: Long, pane: Int, data: ByteArray)
    @JvmStatic external fun linkRequest(h: Long, method: String, path: String, body: String?): String
    @JvmStatic external fun linkFree(h: Long)

    @JvmStatic external fun shown(atId: String?, atPersonal: Boolean, sessionProject: String?, onIssueProject: String?, yours: Boolean): Boolean
    @JvmStatic external fun machines(input: String): String
    @JvmStatic external fun keptPane(path: String, project: String): String?
    @JvmStatic external fun panePath(project: String, machine: String, pane: Int): String?

    @JvmStatic external fun chatOpen(control: Long, notify: Notify): Long
    @JvmStatic external fun chatNext(h: Long): String?
    @JvmStatic external fun chatFree(h: Long)

    @JvmStatic external fun termNew(cols: Int, rows: Int): Long
    @JvmStatic external fun termFree(h: Long)
    @JvmStatic external fun termResize(h: Long, cols: Int, rows: Int, cellW: Int, cellH: Int)
    @JvmStatic external fun termScroll(h: Long, delta: Int)
    @JvmStatic external fun termScrollBottom(h: Long)
    @JvmStatic external fun termKey(h: Long, code: Int, mods: Int, text: String?): ByteArray
    @JvmStatic external fun termRender(h: Long): ByteArray
}
