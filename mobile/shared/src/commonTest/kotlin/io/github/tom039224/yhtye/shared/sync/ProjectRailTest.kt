package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.DomainConfig
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** レールに並べるプロジェクト (開いているもの + 表示中のもの。点は `projectRing`)。 */
class ProjectRailTest {
    private fun info(id: String, open: Boolean = true, running: Boolean = false, awake: Boolean = false) =
        ProjectInfo(id, id, "/work/$id", open, running, awake)

    private fun viewOf(p: ProjectInfo, sessions: List<SessionRecord> = emptyList()): ProjectView =
        newProjectView(p).copy(state = emptyState(p.id, DomainConfig(2)), sessions = sessions)

    private fun orchestrator(turnRunning: Boolean = false) = sessionRecord("orchestrator:C-1", SessionStatus.Live, turnRunning)

    @Test
    fun listsTheOpenProjectsInTheCoresOrder() {
        val projects = listOf(info("c"), info("a", open = false), info("b"), info("d", open = false))
        assertEquals(listOf("c", "b"), openedProjects(projects, currentId = null).map { it.id })
    }

    @Test
    fun alsoListsTheProjectOnScreenEvenIfTheListSaysItIsNotOpenYet() {
        val projects = listOf(info("a"), info("b", open = false), info("c", open = false))
        assertEquals(listOf("a", "b"), openedProjects(projects, currentId = "b").map { it.id })
    }

    @Test
    fun aProjectOnScreenThatIsNotInTheListIsNotInvented() {
        assertEquals(listOf("a"), openedProjects(listOf(info("a")), currentId = "zzz").map { it.id })
        assertTrue(openedProjects(emptyList(), currentId = "a").isEmpty())
    }

    @Test
    fun noOpenProjectsMeansAnEmptyRail() {
        assertTrue(railProjects(listOf(info("a", open = false)), null).isEmpty())
    }

    @Test
    fun otherProjectsAreJudgedByWhatTheCoreLastSaid() {
        val rail = railProjects(
            listOf(info("a", running = true, awake = true), info("b", awake = true), info("c")),
            view = null,
        )
        assertEquals(listOf(Ring.Running, Ring.Ready, Ring.Stopped), rail.map { it.ring })
        assertEquals(listOf(true, false, false), rail.map { it.working })
        assertTrue(rail.none { it.current })
    }

    @Test
    fun theProjectOnScreenIsJudgedByItsLiveView() {
        // 一覧では止まっているが、ビューでは (別のクライアントの操作で) ターンが動いている
        val a = info("a")
        val b = info("b", running = true, awake = true)
        val rail = railProjects(listOf(a, b), viewOf(a, listOf(orchestrator(turnRunning = true))))
        assertEquals(listOf(true, false), rail.map { it.current })
        assertEquals(listOf(Ring.Running, Ring.Running), rail.map { it.ring })

        val idle = railProjects(listOf(a, b), viewOf(a, listOf(orchestrator(turnRunning = false))))
        assertEquals(Ring.Ready, idle[0].ring)
        assertFalse(idle[0].working)
        assertTrue(idle[1].working)
        assertEquals(Ring.Stopped, railProjects(listOf(a), viewOf(a))[0].ring)
    }

    @Test
    fun theDotMatchesProjectRingForEveryProject() {
        val projects = listOf(info("a", running = true), info("b", awake = true), info("c"), info("d", running = true, awake = true))
        val view = viewOf(projects[2], listOf(orchestrator()))
        railProjects(projects, view).forEach { assertEquals(projectRing(it.project, view), it.ring) }
    }

    @Test
    fun anyWorkingTellsWhetherADotIsSpinning() {
        assertFalse(anyWorking(railProjects(listOf(info("a", awake = true), info("b")), null)))
        assertTrue(anyWorking(railProjects(listOf(info("a", awake = true), info("b", running = true)), null)))
        assertFalse(anyWorking(emptyList()))
    }
}
