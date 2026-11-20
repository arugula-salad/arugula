package io.arugula.core

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * One pane's terminal on the phone, on libghostty-vt: [Link.feed] gives it
 * the pane's bytes, and [render] what to draw.
 */
class Term(cols: Int, rows: Int) : AutoCloseable {
    internal var h = Native.termNew(cols, rows)
        private set

    init {
        check(h != 0L) { "the terminal couldn't start" }
    }

    fun resize(cols: Int, rows: Int, cellW: Int, cellH: Int) = Native.termResize(h, cols, rows, cellW, cellH)

    /** Scroll the view back (negative) or forward, in rows. */
    fun scroll(delta: Int) = Native.termScroll(h, delta)

    /** Back to the live screen. */
    fun scrollToBottom() = Native.termScrollBottom(h)

    /** The bytes for a key under the terminal's modes, for [Link.input]. [code] is one of [Key]'s; [mods] [Mods]' bits. */
    fun key(code: Int, mods: Int = 0, text: String? = null): ByteArray = Native.termKey(h, code, mods, text)

    /** The rows that changed since the last call (all of them when [Frame.dirty] is [Frame.FULL]), and the cursor. */
    fun render(): Frame = Frame.decode(Native.termRender(h))

    override fun close() {
        if (h != 0L) Native.termFree(h).also { h = 0 }
    }
}

/** `aru_term_key`'s codes; 'A'..'Z' are themselves. */
object Key {
    const val ESCAPE = 1
    const val TAB = 2
    const val ENTER = 3
    const val BACKSPACE = 4
    const val UP = 5
    const val DOWN = 6
    const val LEFT = 7
    const val RIGHT = 8
    const val HOME = 9
    const val END = 10
    const val PAGE_UP = 11
    const val PAGE_DOWN = 12
    const val DELETE = 13
}

object Mods {
    const val SHIFT = 1
    const val CTRL = 2
    const val ALT = 4
}

/** Cells of one row with the same colors and flags; outside ASCII, one cell each, so wide characters stay on the grid. */
data class Run(
    val col: Int,
    val cells: Int,
    val fg: Int,
    /** 0xRRGGBB, or [Frame.DEFAULT_BG]. */
    val bg: Int,
    /** [BOLD], [ITALIC]. */
    val flags: Int,
    val text: String,
) {
    companion object {
        const val BOLD = 1
        const val ITALIC = 2
    }
}

data class Row(val y: Int, val runs: List<Run>) {
    val text get() = runs.joinToString("") { it.text }
}

data class Frame(
    val dirty: Int,
    val cols: Int,
    val rows: Int,
    val fg: Int,
    val bg: Int,
    val cursorVisible: Boolean,
    val cursorX: Int,
    val cursorY: Int,
    val cursorColor: Int,
    val changed: List<Row>,
) {
    companion object {
        const val CLEAN = 0
        const val PARTIAL = 1
        const val FULL = 2
        const val DEFAULT_BG = -1

        /** From `termRender`'s bytes (kotlin.rs's `frame`). */
        fun decode(bytes: ByteArray): Frame {
            val b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            fun u16() = b.short.toInt() and 0xffff
            val dirty = b.get().toInt()
            val cursorVisible = b.get().toInt() != 0
            val cols = u16()
            val rows = u16()
            val cursorX = u16()
            val cursorY = u16()
            val fg = b.int
            val bg = b.int
            val cursorColor = b.int
            val changed = List(u16()) {
                val y = u16()
                Row(y, List(u16()) {
                    val col = u16()
                    val cells = u16()
                    val rfg = b.int
                    val rbg = b.int
                    val flags = b.get().toInt()
                    val text = ByteArray(u16()).also { b.get(it) }
                    Run(col, cells, rfg, rbg, flags, String(text, Charsets.UTF_8))
                })
            }
            return Frame(dirty, cols, rows, fg, bg, cursorVisible, cursorX, cursorY, cursorColor, changed)
        }
    }
}
