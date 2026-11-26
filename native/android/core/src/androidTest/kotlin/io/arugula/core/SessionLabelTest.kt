package io.arugula.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/** A chief of staff's session, named by its project's id, as people read it
 * (Arugula #1457), through the core's JNI side, with no network. */
@RunWith(AndroidJUnit4::class)
class SessionLabelTest {
    @Test
    fun aChiefOfStaffReadsByItsProjectsNameAndNoOtherSessionChanges() {
        val chief = "Chief of staff · prj_3e5d80aa"
        assertEquals("Chief of staff · Launch crew", Core.sessionLabel(chief, "prj_3e5d80aa", "Launch crew"))
        // Another project's, or none known: as it is.
        assertEquals(chief, Core.sessionLabel(chief, "prj_77", "Other crew"))
        assertEquals(chief, Core.sessionLabel(chief, null, null))
        assertEquals("fix-1457", Core.sessionLabel("fix-1457", "prj_3e5d80aa", "Launch crew"))
    }
}
