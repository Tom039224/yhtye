package io.github.tom039224.yhtye.shared.sync

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** PC の `src/store/names.test.ts` の移植。 */
class NamesTest {
    private val long = "x".repeat(32)

    @Test
    fun `names of at most 32 characters and the older numbered ids are ids`() {
        for (ok in listOf("a", "headless-server", "v2", "a-1-b", long)) assertTrue(isName(ok), ok)
        for (bad in listOf("", "-a", "a-", "a--b", "Headless", "G-1", "a_b", "a/b", "${long}x")) assertFalse(isName(bad), bad)
        assertTrue(isGroupId("G-12"))
        assertFalse(isGroupId("T-12"))
        assertTrue(isTaskId("T-12"))
        assertFalse(isTaskId("G-12"))
        assertFalse(isTaskId("T-"))
    }

    @Test
    fun `named group and task branches are read`() {
        assertEquals(YhtyeBranch.Group("headless-server"), parseYhtyeBranch("yhtye/headless-server"))
        assertEquals(YhtyeBranch.Task("headless-server", "remote-icon"), parseYhtyeBranch("yhtye/headless-server--remote-icon"))
        assertEquals(YhtyeBranch.Task(long, long), parseYhtyeBranch("yhtye/$long--$long"))
    }

    @Test
    fun `the older numbered branches are still read`() {
        assertEquals(YhtyeBranch.Group("G-1"), parseYhtyeBranch("yhtye/G-1"))
        assertEquals(YhtyeBranch.Task("G-1", "T-2"), parseYhtyeBranch("yhtye/G-1-T-2"))
        assertEquals(YhtyeBranch.Task("G-46", "T-120"), parseYhtyeBranch("yhtye/G-46-T-120"))
    }

    @Test
    fun `a named task in a numbered group is read`() {
        assertEquals(YhtyeBranch.Task("G-46", "remote-icon"), parseYhtyeBranch("yhtye/G-46--remote-icon"))
    }

    @Test
    fun `a name ending in t-n is not a numbered task`() {
        assertEquals(YhtyeBranch.Group("fix-t-3"), parseYhtyeBranch("yhtye/fix-t-3"))
    }

    @Test
    fun `other branches are null`() {
        val others = listOf(
            "main",
            "feature/x",
            "yhtye/",
            "yhtye/Feature",
            "yhtye/a/b",
            "yhtye/a--",
            "yhtye/--a",
            "yhtye/a--b--c",
            "yhtye/a--T-1",
            "yhtye/${long}x",
            "yhtye/G-1-T-",
            "xyhtye/G-1",
        )
        for (other in others) assertNull(parseYhtyeBranch(other), other)
    }
}
