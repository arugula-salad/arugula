package io.arugula.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The core on an Android emulator, through its Kotlin side, as the app (#1249)
 * will use it, against a real control and machine: native/android/test-emulator.sh
 * runs it, and the cloud repository's testnet `native-android` claim drives that.
 *
 * Arguments (`-e`): `control` (control's address, as this device reaches it),
 * `machine` (a machine's name there), `room` (a chat room). Something else
 * allows the sign-in: the test writes its ticket to `files/handoff/ticket` and
 * waits for the address the Allow was sent to in `files/handoff/callback`.
 */
@RunWith(AndroidJUnit4::class)
class FlowTest {
    private val args = InstrumentationRegistry.getArguments()
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun init() = Core.init(context)

    @Test
    fun signsInListsProjectsChatsAndReadsAPane() {
        val url = args.getString("control")
        assumeTrue("no control given (-e control URL)", url != null)
        val machine = args.getString("machine") ?: "box-systemd"
        val stored = signIn(url!!)

        Control(stored).use { c ->
            // The startup call: who it is, and what it has.
            val start = JSONObject(c.get("/api/projects"))
            assertTrue("no projects: $start", start.has("projects"))

            args.getString("room")?.let { chat(c, it) }
            pane(c, machine)
            c.signOut()
            val after = runCatching { c.get("/api/projects") }.exceptionOrNull()
            assertTrue("still signed in after signing out: $after", (after as? CoreError)?.signedOut == true)
        }
    }

    /**
     * TLS to the real Arugula, checked against Android's trust store: it
     * serves this build, and a made-up session is refused by control, not by TLS.
     */
    @Test
    fun tlsIsCheckedByAndroid() {
        assumeTrue("TLS check off (-e tls off)", args.getString("tls") != "off")
        Core.checkApp(ARUGULA)
        val made = JSONObject(mapOf("url" to ARUGULA, "cookie" to "arugula_session=none", "account" to "nobody"))
        Control(made.toString()).use { c ->
            val e = runCatching { c.get("/api/projects") }.exceptionOrNull()
            assertTrue("expected control's refusal, got $e", e is CoreError && e.status == 401)
        }
    }

    private companion object {
        const val ARUGULA = "https://control.arugula.io"
    }

    /** The CLI's way: the ticket goes to whoever allows it, and the callback comes back. */
    private fun signIn(url: String): String {
        val handoff = File(context.filesDir, "handoff").apply { deleteRecursively(); mkdirs() }
        return SignIn.start(url, "Arugula on Android (test)").use { s ->
            assertTrue("not control's Allow page: ${s.link}", s.link.startsWith("http"))
            File(handoff, "ticket").writeText(s.ticket)
            val callback = File(handoff, "callback")
            val end = System.currentTimeMillis() + minOf(s.expiresInSecs, 120) * 1000
            while (!callback.exists() || callback.readText().isBlank()) {
                assertTrue("nobody allowed it", System.currentTimeMillis() < end)
                Thread.sleep(300)
            }
            val to = callback.readText().trim()
            assertTrue("not the app's callback: $to", to.startsWith("${Core.CALLBACK_SCHEME}://signin?"))
            s.finish(to).also { assertTrue(JSONObject(it).getString("cookie").isNotEmpty()) }
        }
    }

    /** Chat's live stream says hello from its own thread; a post is read back. */
    private fun chat(c: Control, room: String) {
        val ready = LinkedBlockingQueue<Unit>()
        ChatLive(c) { ready.offer(Unit) }.use { live ->
            assertNotNull("the live stream never said anything", ready.poll(30, TimeUnit.SECONDS))
            val first = JSONObject(live.next() ?: error("told, and nothing there"))
            assertEquals("hello", first.getString("type"))
        }
        val marker = "ANDROID-${System.nanoTime()}"
        c.post("/api/chat/rooms/$room/messages", JSONObject(mapOf("text" to marker)).toString())
        assertTrue("$marker isn't in the room", c.get("/api/chat/rooms/$room/messages").contains(marker))
    }

    /** Through the relay: a new pane, typed into, its answer read off this phone's own terminal. */
    private fun pane(c: Control, name: String) {
        val daemons = JSONObject(c.get("/api/directory")).getJSONArray("daemons")
        val id = (0 until daemons.length()).map { daemons.getJSONObject(it) }
            .firstOrNull { it.getString("name") == name }?.getString("id")
        assertNotNull("$name isn't in the directory", id)

        val ready = LinkedBlockingQueue<Unit>()
        Link(c, id!!) { ready.offer(Unit) }.use { link ->
            Term(100, 30).use { term ->
                // Its hello first: nothing's attached until it has said who's here.
                waitFor(ready, "the machine's hello") {
                    link.feed()
                    link.panes != null
                }
                val ran = JSONObject(link.request("POST", "/api/run", "{}"))
                assertEquals(200, ran.getInt("status") / 100 * 100)
                val pane = ran.getJSONObject("body").getInt("pane")

                var tab = -1
                waitFor(ready, "pane %$pane") {
                    link.feed()
                    tab = tabOf(link.panes, pane)
                    tab >= 0
                }
                link.attach(tab, pane, 100, 30)
                val marker = "ANDROID-${System.nanoTime() % 100000}"
                link.input(pane, "echo $marker-\$((6*7))".toByteArray())
                link.input(pane, term.key(Key.ENTER))

                val screen = sortedMapOf<Int, String>()
                waitFor(ready, "$marker-42 on the terminal") {
                    if (link.feed(intArrayOf(pane), arrayOf(term)) and Link.CLOSED != 0) error("closed: ${link.closed}")
                    term.render().changed.forEach { screen[it.y] = it.text }
                    // The answer on a line of its own, below the command.
                    screen.values.any { it.trim() == "$marker-42" }
                }
            }
        }
    }

    private fun tabOf(panes: String?, pane: Int): Int {
        val list = JSONObject(panes ?: return -1).getJSONArray("panes")
        return (0 until list.length()).map { list.getJSONObject(it) }.firstOrNull { it.getInt("id") == pane }?.getInt("tab") ?: -1
    }

    /** Check [done] each time the core says there's something, until it holds. */
    private fun waitFor(ready: LinkedBlockingQueue<Unit>, what: String, done: () -> Boolean) {
        val end = System.currentTimeMillis() + 30_000
        while (!done()) {
            val left = end - System.currentTimeMillis()
            assertTrue("$what never came", left > 0)
            ready.poll(left, TimeUnit.MILLISECONDS)
        }
    }
}
