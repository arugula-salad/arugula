// Control's requests with the session, off the main thread, and the JSON the
// app reads from them: only the fields it shows, as the iPhone's Models.swift,
// so anything else control adds changes nothing here.

package io.arugula.app

import io.arugula.core.Control
import io.arugula.core.Core
import io.arugula.core.CoreError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

class Api(val control: Control, private val signedOut: () -> Unit) {
    suspend fun call(method: String, path: String, body: JSONObject? = null): String = withContext(Dispatchers.IO) {
        try {
            control.call(method, path, body?.toString())
        } catch (e: CoreError) {
            if (e.signedOut) signedOut()
            throw e
        }
    }

    suspend fun get(path: String) = JSONObject(call("GET", path))

    suspend fun post(path: String, body: JSONObject = JSONObject()) = call("POST", path, body).let {
        runCatching { JSONObject(it) }.getOrDefault(JSONObject())
    }

    /** The thread of an object (a board, an issue): made the first time someone opens it. */
    suspend fun threadOf(target: String) =
        ChatRoom.of(post("/api/chat/targets", JSONObject().put("machine", "").put("kind", "object").put("target", target)))
}

/** What control says when something fails, in its own words. */
fun describe(e: Throwable): String = e.message ?: e.toString()

// ---- reading JSON

fun JSONObject.str(k: String): String? = if (isNull(k)) null else optString(k)
fun JSONObject.int(k: String): Int? = if (isNull(k) || !has(k)) null else optInt(k)
fun JSONObject.long(k: String): Long? = if (isNull(k) || !has(k)) null else optLong(k)
fun JSONObject.bool(k: String): Boolean? = if (isNull(k) || !has(k)) null else optBoolean(k)
fun JSONObject.obj(k: String): JSONObject? = optJSONObject(k)
fun <T> JSONObject.list(k: String, f: (JSONObject) -> T): List<T> = optJSONArray(k).objects().mapNotNull { runCatching { f(it) }.getOrNull() }
fun JSONArray?.objects(): List<JSONObject> = if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }
fun JSONArray?.strings(): List<String> = if (this == null) emptyList() else (0 until length()).map { optString(it) }
fun JSONObject.ints(k: String): List<Int> = optJSONArray(k).let { a -> if (a == null) emptyList() else (0 until a.length()).map { a.optInt(it) } }

// ---- projects, issues, boards

data class Team(val id: String, val name: String, val role: String)

data class Project(val id: String, val name: String, val team: Team?) {
    companion object {
        fun of(o: JSONObject) = Project(
            o.getString("id"), o.getString("name"),
            o.obj("team")?.let { Team(it.getString("id"), it.getString("name"), it.optString("role")) },
        )
    }
}

data class Me(val account: String, val login: String, val name: String)

data class Projects(val me: Me, val projects: List<Project>) {
    companion object {
        fun of(o: JSONObject) = Projects(
            o.getJSONObject("me").let { Me(it.optString("account"), it.optString("login"), it.optString("name")) },
            o.list("projects", Project::of),
        )
    }
}

data class Issue(val id: String, val number: Int, val title: String, val body: String, val state: String, val labels: List<String>) {
    companion object {
        fun of(o: JSONObject) = Issue(
            o.getString("id"), o.getInt("number"), o.getString("title"), o.optString("body"), o.optString("state"),
            o.optJSONArray("labels").strings(),
        )
    }
}

data class Lead(val machine: String, val machineName: String?, val session: String)

data class Progress(val done: Int, val review: Int, val doing: Int, val total: Int)

data class Board(val id: String, val title: String, val goal: String?, val progress: Progress?, val closedAt: Long?, val lead: Lead?) {
    companion object {
        fun of(o: JSONObject) = Board(
            o.getString("id"), o.getString("title"), o.str("goal"),
            o.obj("progress")?.let { Progress(it.optInt("done"), it.optInt("review"), it.optInt("doing"), it.optInt("total")) },
            o.long("closed_at"), lead(o.obj("lead")),
        )

        fun lead(o: JSONObject?) = o?.let { Lead(it.optString("machine"), it.str("machine_name"), it.optString("session")) }
    }
}

/** One board: its lanes (Next, Doing, Review, Done) with their rows, and its groups. */
data class BoardDetail(val title: String, val goal: String?, val lead: Lead?, val lanes: List<Lane>, val groups: Map<String, String>) {
    /** The issue's or PR's state as the board read it. */
    data class Live(val kind: String?, val number: Int?, val title: String?, val state: String?, val repo: String?)

    /** What a row points at; a block names its machine and pane. */
    data class Pointer(val kind: String, val id: String?, val url: String?, val machine: String?, val pane: String?)

    /** The row's one open question: [mine] when it's for the reader. */
    /** [who] is the person it asks (empty: the board raised it), [whoName] their name. */
    data class Ask(val id: String, val question: String, val options: List<String>, val mine: Boolean, val canAnswer: Boolean?, val askedBy: String?, val createdAt: Long?, val who: String? = null, val whoName: String? = null)

    data class Attachment(val role: String, val pointer: Pointer, val released: Boolean, val live: Live?)

    data class Row(
        val id: String, val note: String?, val group: String?, val lane: String?, val live: Live?, val pointer: Pointer?,
        val ask: Ask?, val attachments: List<Attachment>, val marks: List<String>, val updatedAt: Long?,
    )

    data class Lane(val id: String, val title: String, val rows: List<Row>)

    fun groupTitle(id: String?) = id?.let { groups[it] }

    companion object {
        private fun live(o: JSONObject?) = o?.let { Live(it.str("kind"), it.int("number"), it.str("title"), it.str("state"), it.str("repo")) }
        private fun pointer(o: JSONObject?) = o?.let { Pointer(it.optString("kind"), it.str("id"), it.str("url"), it.str("machine"), it.str("pane")) }

        private fun row(o: JSONObject) = Row(
            o.getString("id"), o.str("note"), o.str("group"), o.str("lane"), live(o.obj("live")), pointer(o.obj("pointer")),
            o.obj("ask")?.let {
                Ask(it.getString("id"), it.optString("question"), it.optJSONArray("options").strings(), it.optBoolean("mine"), it.bool("can_answer"), it.str("asked_by"), it.long("created_at"), it.str("who"), it.str("who_name"))
            },
            o.list("attachments") { Attachment(it.optString("role"), pointer(it.getJSONObject("pointer"))!!, it.optBoolean("released"), live(it.obj("live"))) },
            o.list("marks") { it.optString("kind") },
            o.long("updated_at"),
        )

        fun of(o: JSONObject): BoardDetail {
            val b = o.getJSONObject("board")
            return BoardDetail(
                b.optString("title"), b.str("goal"), Board.lead(b.obj("lead")),
                o.list("zones") { z -> Lane(z.getString("id"), z.optString("title"), z.list("rows", ::row)) },
                o.optJSONArray("groups").objects().associate { it.getString("id") to it.optString("title") },
            )
        }
    }
}

// ---- chat

data class ChatRoom(
    val id: String, val kind: String, val name: String, val last: Long, val unread: Int, val mentions: Int,
    val archived: Boolean, val writable: Boolean, val team: String?, val with: String?, val others: List<String>,
) {
    /** A channel or a DM: what the chat list shows. An issue's, a pane's or a session's thread belongs with its thing. */
    val isConversation get() = kind == "channel" || kind == "dm"

    /** A channel by its name, a DM by its name if it was given one, else by the people in it. */
    val title: String
        get() = when (kind) {
            "channel" -> "# $name"
            "dm" -> name.ifEmpty { others.joinToString(", ").ifEmpty { with ?: "Direct message" } }
            else -> name
        }

    companion object {
        fun of(o: JSONObject) = ChatRoom(
            o.getString("id"), o.optString("kind"), o.optString("name"), o.optLong("last"), o.optInt("unread"),
            o.optInt("mentions"), o.optBoolean("archived"), o.optBoolean("writable", true), o.str("team"),
            o.obj("with")?.optString("name"), o.list("others") { it.optString("name") },
        )

        /** A thread to read by its id, as a session's or an idea's. */
        fun thread(id: String, name: String) = ChatRoom(id, "thread", name, 0, 0, 0, false, true, null, null, emptyList())
    }
}

data class Reaction(val emoji: String, val count: Int, val mine: Boolean)

data class ChatMessage(
    val seq: Long, val at: Long, val author: String?, val name: String, val text: String, val quote: String?,
    val deleted: Boolean, val edited: Boolean, val agentMachine: String?, val via: String?, val reactions: List<Reaction>,
    val files: List<String>,
) {
    /** Who it's from, as the web says it. */
    val from: String
        get() = when {
            agentMachine != null -> "$name's agent on $agentMachine"
            !via.isNullOrEmpty() && name.isNotEmpty() -> "$name via $via"
            else -> name.ifEmpty { "Arugula" }
        }

    companion object {
        fun of(o: JSONObject) = ChatMessage(
            o.getLong("seq"), o.optLong("at"), o.str("author"), o.optString("name"), o.optString("text"), o.str("quote"),
            o.long("deleted") != null, o.long("edited") != null, o.obj("agent")?.optString("machine_name"), o.str("via"),
            o.list("reactions") { Reaction(it.optString("emoji"), it.optInt("count"), it.optBoolean("mine")) },
            o.list("files") { it.optString("name") },
        )
    }
}

// ---- machines

/**
 * A machine in the directory. [ownerName]: whose it is, when it's someone
 * else's (M19); [account] is set whenever it isn't yours, as the web's fleet,
 * even when the owner has no name to show.
 */
data class Machine(val id: String, val name: String, val online: Boolean, val ownerName: String? = null, val account: String? = null) {
    val yours get() = account == null && ownerName == null

    companion object {
        fun of(o: JSONObject) = Machine(
            o.getString("id"), o.optString("name"), o.optBoolean("online"),
            o.str("owner_name")?.takeIf { it.isNotEmpty() }, o.str("account")?.takeIf { it.isNotEmpty() },
        )
    }
}

/** The project the app shows, as the core's rule reads it (Arugula #1355): Machines and Home show only its sessions. */
data class Shown(val id: String, val personal: Boolean)

val Project.shown get() = Shown(id, team == null)

/**
 * Whether a pane (its session's project and its card's) or a session (no
 * card) on a machine that's [yours] or not shows here: the core's rule, the
 * web's `inShown`. With no project, every one does.
 */
fun Shown?.shows(project: String?, card: String? = null, yours: Boolean) =
    Core.shown(this?.id, this?.personal ?: false, project, card, yours)

/** One of a machine's sessions the core keeps for a project: its tabs' panes, and how many need you. */
data class SessionCard(val id: Int, val tabs: Map<Int, List<Int>>, val needsYou: Int) {
    val panes get() = tabs.values.flatten()
}

/**
 * The core's `aru_machines` for [hosts] (each `{id, name, online, owner,
 * panes}`): every machine in the phone's order, and each one's sessions
 * that show in [shown]. [recent] is the machine used last.
 */
fun sections(hosts: List<JSONObject>, shown: Shown?, recent: String?): JSONObject? = runCatching {
    val input = JSONObject().put("hosts", JSONArray(hosts))
    shown?.let { input.put("at", JSONObject().put("id", it.id).put("personal", it.personal)) }
    recent?.let { input.put("recent", it) }
    Core.machines(input)
}.getOrNull()

fun Machine.host(panes: String? = null): JSONObject = JSONObject().put("id", id).put("name", name).put("online", online).apply {
    (ownerName ?: account)?.let { put("owner", it) }
    panes?.let { runCatching { put("panes", JSONObject(it)) } }
}

/** A project's agent sessions in Arugula cloud, newest first: what each waits on the reader for. */
data class SessionRow(
    val session: String, val room: String, val parent: Long, val state: String, val issueId: String?, val issueNumber: Int?,
    val issueTitle: String?, val waiting: String?, val question: String?, val updatedAt: Long,
) {
    val thread get() = ChatRoom.thread(room, issueNumber?.let { "#$it $issueTitle" } ?: "Session")

    val standing: Standing
        get() = when {
            waiting != null -> Standing.NEEDS
            state in listOf("failed", "error", "unresponsive") -> Standing.FAILED
            state in listOf("working", "running", "queued") -> Standing.WORKING
            else -> Standing.QUIET
        }

    companion object {
        fun of(o: JSONObject): SessionRow {
            val i = o.obj("issue")
            return SessionRow(
                o.getString("session"), o.getString("room"), o.optLong("parent"), o.optString("state"), i?.str("id"),
                i?.int("number"), i?.str("title"), o.str("waiting"), o.str("question"), o.optLong("updated_at"),
            )
        }
    }
}

/** One hosted session: what it asks now, whether the reader may answer it, and its history in words. */
data class SessionView(
    val state: String, val request: String?, val ask: String?, val question: String?, val options: List<String>,
    val open: Boolean, val answered: Boolean, val may: Boolean, val events: List<String>,
) {
    companion object {
        fun of(o: JSONObject): SessionView {
            val card = o.getJSONObject("card")
            val open = card.obj("open")
            return SessionView(
                card.optString("state"), open?.str("request"), open?.str("ask"), open?.str("question"),
                open?.optJSONArray("options").strings(), open != null, card.obj("answered") != null, o.optBoolean("may"),
                o.list("events") { it.optString("text") },
            )
        }
    }
}
