package io.arugula.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/** libghostty-vt on the device, through Kotlin, with no network. */
@RunWith(AndroidJUnit4::class)
class TermTest {
    @Test
    fun aKeyIsItsBytesUnderTheTerminalsModes() = Term(80, 24).use { t ->
        assertArrayEquals(byteArrayOf(13), t.key(Key.ENTER))
        assertArrayEquals("a".toByteArray(), t.key('A'.code, 0, "a"))
        assertArrayEquals(byteArrayOf(3), t.key('C'.code, Mods.CTRL, "c"))
    }

    @Test
    fun theFirstFrameIsWholeAndTheNextIsClean() = Term(80, 24).use { t ->
        val f = t.render()
        assertEquals(Frame.FULL, f.dirty)
        assertEquals(80, f.cols)
        assertEquals(24, f.rows)
        assertEquals(24, f.changed.size)
        assertEquals(Frame.CLEAN, t.render().dirty)
    }
}
