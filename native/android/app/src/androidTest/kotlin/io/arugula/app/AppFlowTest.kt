package io.arugula.app

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The app itself on an emulator, tapped through as a person would, against a
 * real control and machine: native/android/test-app.sh runs it, and the cloud
 * repository's testnet `native-android` claim drives that. The same flow as
 * the iPhone's FlowTests.swift.
 *
 * Arguments (`-e`): `control` (control's address, as this device reaches it),
 * `machine` (a machine's name there), `room` (a chat room's id), and `board`
 * and `ask` (a board in the personal project, and its row's question to the
 * person signing in). Something else allows the sign-in: the app writes its
 * ticket to `files/handoff/ticket`, and the test waits for the address the
 * Allow was sent to in `files/handoff/callback` and types it in. Screenshots
 * go to `files/handoff` as PNGs.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class AppFlowTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private val args = InstrumentationRegistry.getArguments()
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val handoff = File(instrumentation.targetContext.filesDir, "handoff")

    @Test
    fun signsInGoesThroughTheTabsChatsAndTypesInATerminal() {
        val url = args.getString("control")
        assumeTrue("no control given (-e control URL)", url != null)
        val machine = args.getString("machine") ?: "box-systemd"
        handoff.deleteRecursively()
        handoff.mkdirs()

        val intent = Intent(instrumentation.targetContext, MainActivity::class.java)
            .putExtra("control", url).putExtra("signin_by_hand", true).putExtra("reset", true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ActivityScenario.launch<MainActivity>(intent).use {
            // Sign in: the ticket goes to whoever allows it, and the address comes back.
            tap("signin-start")
            wait("signin-ticket")
            shot("1-signin")
            val callback = File(handoff, "callback")
            val until = System.currentTimeMillis() + 120_000
            while (!callback.exists() || callback.readText().isBlank()) {
                check(System.currentTimeMillis() < until) { "nobody allowed the sign-in" }
                Thread.sleep(500)
            }
            rule.onNode(hasTestTag("signin-callback")).performTextInput(callback.readText().trim())
            tap("signin-finish")

            // The Overview, with the web's five places on the bar under it
            // (Arugula #1362), and the bell and your picture in its heading.
            wait("home-list", 30_000)
            for (name in listOf("Overview", "Boards", "Docs", "Machines", "Chat")) wait("tab-$name")
            for (gone in listOf("Home", "You")) check(rule.onAllNodes(hasTestTag("tab-$gone")).fetchSemanticsNodes().isEmpty()) { "$gone is still a tab" }
            wait("heading-new")
            wait("heading-inbox")
            wait("account")
            args.getString("board")?.takeIf { it.isNotEmpty() }?.let { board -> home(board, args.getString("ask")) }
            shot("2-home")

            // The project's issues, from its menu.
            tap("project-switcher")
            tap("project-issues")
            until("the issues", 15_000) { rule.onAllNodes(hasText("Issues", substring = true)).fetchSemanticsNodes().isNotEmpty() }
            tap("back")

            // Boards: the board's card opens its lanes.
            tap("tab-Boards")
            args.getString("board")?.takeIf { it.isNotEmpty() }?.let { board ->
                wait("board-$board", 20_000)
                tap("board-$board")
                until("the lanes", 15_000) { rule.onAllNodes(hasText("NEXT")).fetchSemanticsNodes().isNotEmpty() }
                shot("3-board")
                // The bar is still there.
                wait("tab-Overview")
            }

            // Chat: the room, a message posted from the phone.
            args.getString("room")?.let { room -> chat(room) }

            // Machines: a new terminal on the machine, typed into, its answer on screen.
            tap("tab-Machines")
            // A machine with nothing in the project is folded into one line.
            until("the machine or the fold", 20_000) {
                listOf("machine-$machine", "machines-quiet").any { rule.onAllNodes(hasTestTag(it)).fetchSemanticsNodes().isNotEmpty() }
            }
            if (rule.onAllNodes(hasTestTag("machine-$machine")).fetchSemanticsNodes().isEmpty()) {
                rule.onNode(hasTestTag("machines-list")).performScrollToNode(hasTestTag("machines-quiet"))
                tap("machines-quiet")
            }
            rule.onNode(hasTestTag("machines-list")).performScrollToNode(hasTestTag("machine-$machine"))
            wait("machine-$machine", 20_000)
            tap("machine-$machine")
            until("New terminal enabled", 60_000) {
                rule.onAllNodes(hasTestTag("new-terminal")).fetchSemanticsNodes().any { n ->
                    !n.config.contains(SemanticsProperties.Disabled)
                }
            }
            tap("new-terminal")
            wait("terminal", 20_000)
            Thread.sleep(2_000)
            rule.onNode(hasTestTag("term-input")).performTextInput("echo ANDROID-$((6*7))\n")
            until("ANDROID-42 in the terminal", 30_000) { terminalText().contains("ANDROID-42") }
            shot("5-terminal")

            // Docs (Arugula #1364): the project's Docs, newest first, or a
            // plain word that it has none yet. One opens in its web view.
            tap("tab-Docs")
            wait("docs-place")
            val docRow = SemanticsMatcher("a Doc's row") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("doc-row-") == true }
            until("a Doc or the word there are none", 30_000) {
                listOf(hasTestTag("docs-none"), docRow).any { rule.onAllNodes(it).fetchSemanticsNodes().isNotEmpty() }
            }
            shot("6-docs")
            if (rule.onAllNodes(docRow).fetchSemanticsNodes().isNotEmpty()) {
                rule.onAllNodes(docRow).onFirst().performClick()
                wait("doc-web", 15_000)
                shot("6-doc")
                tap("back")
            }

            // Your picture: sign out, and the app asks for a sign-in again.
            tap("tab-Overview")
            tap("account")
            tap("sign-out")
            wait("signin-start", 15_000)
        }
    }

    /** The Overview shows the board, and its row's question under Needs you, answered there. */
    private fun home(board: String, ask: String?) {
        if (!ask.isNullOrEmpty()) {
            until("the board's question on home", 45_000) { rule.onAllNodes(hasText(ask, substring = true)).fetchSemanticsNodes().isNotEmpty() }
            shot("2-home-asks")
            rule.onAllNodes(hasTestTag("answer-Yes")).onFirst().performClick()
            until("the answered question gone", 20_000) { rule.onAllNodes(hasText(ask, substring = true)).fetchSemanticsNodes().isEmpty() }
        }
        rule.onNode(hasTestTag("home-list")).performScrollToNode(hasTestTag("home-board-$board"))
        tap("home-board-$board")
        // Its thread, then its lanes.
        wait("open-board", 15_000)
        wait("board-lead")
        tap("open-board")
        until("the lanes", 15_000) { rule.onAllNodes(hasText("NEXT")).fetchSemanticsNodes().isNotEmpty() }
        tap("back")
        tap("back")
        wait("home-list")
    }

    private fun chat(room: String) {
        val name = runBlocking { App.model.control!!.get("/api/chat") }.list("rooms", ChatRoom::of).first { it.id == room }.name
        tap("tab-Chat")
        wait("room-$name", 20_000)
        tap("room-$name")
        val marker = "ANDROID-APP-${System.currentTimeMillis() % 100000}"
        wait("chat-draft", 10_000)
        rule.onNode(hasTestTag("chat-draft")).performTextInput(marker)
        tap("chat-send")
        until("the message in the room", 20_000) { rule.onAllNodes(hasText(marker, substring = true)).fetchSemanticsNodes().isNotEmpty() }
        val page = runBlocking { App.model.control!!.call("GET", "/api/chat/rooms/$room/messages") }
        check(page.contains(marker)) { "the message isn't in its room" }
        shot("4-chat")
        tap("back")
    }

    private fun terminalText() = rule.onAllNodes(hasTestTag("terminal")).fetchSemanticsNodes()
        .flatMap { it.config.getOrElse(SemanticsProperties.ContentDescription) { emptyList() } }.joinToString("\n")

    private fun wait(tag: String, ms: Long = 10_000) = until("$tag on screen", ms) {
        rule.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()
    }

    /** [what] comes within [ms], or the test fails saying so, with the screen as it was. */
    private fun until(what: String, ms: Long, ok: () -> Boolean) {
        try {
            rule.waitUntil(ms, ok)
        } catch (e: Throwable) {
            shot("failed")
            throw AssertionError("no $what after $ms ms", e)
        }
    }

    private fun tap(tag: String) {
        wait(tag)
        rule.onAllNodes(hasTestTag(tag)).onFirst().performClick()
        rule.waitForIdle()
    }

    /** The screen as it is, for the PR and the claim's record. */
    private fun shot(name: String) {
        rule.waitForIdle()
        Thread.sleep(600)
        val b = instrumentation.uiAutomation.takeScreenshot() ?: return
        File(handoff, "$name.png").outputStream().use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
