package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainConfig
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** PC の `src/store/activity.test.ts` と `src/ui/projectActivity.test.ts` の移植。 */
class ActivityTest {
    private fun info(id: String, open: Boolean = true, running: Boolean = false) = ProjectInfo(id, id, "/work/$id", open, running)

    private fun session(status: SessionStatus = SessionStatus.Live, turnRunning: Boolean = false): SessionRecord =
        sessionRecord("orchestrator:C-1", status, turnRunning)

    private fun task(status: TaskStatus): Task = FULL_RUN.end.state.tasks[0].copy(status = status)

    private fun viewOf(
        p: ProjectInfo,
        sessions: List<SessionRecord> = emptyList(),
        tasks: List<Task> = emptyList(),
        phase: LoadPhase = LoadPhase.Ready,
    ): ProjectView {
        val state = emptyState(p.id, DomainConfig(2)).copy(tasks = tasks)
        return newProjectView(p).copy(state = state, sessions = sessions, phase = phase)
    }

    // ---- hasRunningWork ----

    @Test
    fun isFalseForAProjectWithNothingGoingOn() {
        assertFalse(hasRunningWork(viewOf(info("a"))))
        assertFalse(hasRunningWork(newProjectView(info("a"))))
    }

    @Test
    fun isTrueWhileALiveSessionIsInATurn() {
        assertTrue(hasRunningWork(viewOf(info("a"), sessions = listOf(session(turnRunning = true)))))
    }

    @Test
    fun ignoresATurnOfASessionThatIsNotLiveAnyMore() {
        assertFalse(hasRunningWork(viewOf(info("a"), sessions = listOf(session(SessionStatus.Stopped, turnRunning = true)))))
    }

    @Test
    fun isTrueWhileATaskIsRunningOrMergingNotOnceItIsSettled() {
        assertTrue(hasRunningWork(viewOf(info("a"), tasks = listOf(task(TaskStatus.Running)))))
        assertTrue(hasRunningWork(viewOf(info("a"), tasks = listOf(task(TaskStatus.Done), task(TaskStatus.Merging)))))
        val settled = listOf(task(TaskStatus.Done), task(TaskStatus.Cancelled), task(TaskStatus.Pending))
        assertFalse(hasRunningWork(viewOf(info("a"), tasks = settled)))
    }

    // ---- changesActivity ----

    @Test
    fun isTrueForATurnStartingOrEndingAndForSessionsComingUpAndGoingAway() {
        assertTrue(changesActivity(prompted(1, ORCHESTRATOR)))
        assertTrue(changesActivity(turnEnded(1, ORCHESTRATOR)))
        assertTrue(changesActivity(ev(1, ApiEventBody.SessionStopped(ORCHESTRATOR, suspended = false))))
        assertTrue(changesActivity(ev(1, ApiEventBody.SessionInterrupted(ORCHESTRATOR))))
        assertTrue(changesActivity(ev(1, ApiEventBody.Agent(ORCHESTRATOR, AgentEvent.Exited(AgentEvent.Exited.Data(1, null))))))
    }

    @Test
    fun isTrueForTheDomainEventsThatMoveATaskInOrOutOfWork() {
        val moving = listOf(
            DomainEvent.TaskCreated(task(TaskStatus.Pending)),
            DomainEvent.TaskStatusChanged("T-1", TaskStatus.Running),
            DomainEvent.TaskCancelled("T-1", "r"),
            DomainEvent.GroupCancelled("G-1", "r"),
            DomainEvent.ChatDeleted("C-1"),
        )
        for (event in moving) assertTrue(changesActivity(domain(1, event)), "$event")
        assertFalse(changesActivity(domain(1, DomainEvent.StepCompleted("T-1", 0, "x", null))))
    }

    @Test
    fun isFalseForTheOutputOfATurnAndForLiveEvents() {
        assertFalse(changesActivity(agentText(1, ORCHESTRATOR, "hi")))
        assertFalse(changesActivity(chunk(1, ORCHESTRATOR, "hi")))
        assertFalse(changesActivity(prompted(1, ORCHESTRATOR).copy(live = true)))
    }

    // ---- mergeActivity ----

    @Test
    fun takesOpenAndRunningOfTheProjectsItKnowsKeepingTheRest() {
        val known = listOf(info("a").copy(name = "A"), info("b", open = false))
        val merged = mergeActivity(known, listOf(info("a", true, true), info("b", true, false), info("c", true, true)))
        assertEquals(listOf(info("a", true, true).copy(name = "A"), info("b", true, false)), merged)
    }

    @Test
    fun isTheSameListWhenNothingChangedAndNeverDropsAProjectTheAnswerLacks() {
        val known = listOf(info("a"), info("b", true, true))
        assertSame(known, mergeActivity(known, listOf(info("b", true, true), info("a"))))
        assertSame(known, mergeActivity(known, emptyList()))
    }

    // ---- rememberRunning ----

    @Test
    fun setsTheFlagOfTheProjectLeftFromWhatItsViewShowed() {
        val a = info("a")
        val busy = viewOf(a, tasks = listOf(task(TaskStatus.Running)))
        assertEquals(listOf(a.copy(running = true), info("b")), rememberRunning(listOf(a, info("b")), busy))
        val idle = viewOf(info("a", true, true))
        assertEquals(listOf(info("a", true, false)), rememberRunning(listOf(info("a", true, true)), idle))
    }

    @Test
    fun leavesTheListAloneWhenThereIsNoViewOrItHasNotLoaded() {
        val projects = listOf(info("a", true, true))
        assertSame(projects, rememberRunning(projects, null))
        assertSame(projects, rememberRunning(projects, viewOf(info("a"), phase = LoadPhase.Loading)))
        assertSame(projects, rememberRunning(projects, viewOf(info("a"), phase = LoadPhase.Error)))
    }

    // ---- projectRing (projectActivity.test.ts) ----

    @Test
    fun isReadyNotStoppedForAProjectThatIsNotOpen() {
        assertEquals(Ring.Ready, projectRing(info("a", open = false), null))
        assertEquals(Ring.Ready, projectRing(info("a", open = false), viewOf(info("b"))))
    }

    @Test
    fun isReadyForAnOpenProjectThatIsIdle() {
        assertEquals(Ring.Ready, projectRing(info("a"), null))
        assertEquals(Ring.Ready, projectRing(info("a"), viewOf(info("a"))))
        assertEquals(Ring.Ready, projectRing(info("a"), viewOf(info("b"))))
    }

    @Test
    fun isRunningForTheProjectOnScreenWhileWorkIsOnInItsView() {
        val a = info("a")
        assertEquals(Ring.Running, projectRing(a, viewOf(a, sessions = listOf(session(turnRunning = true)))))
        assertEquals(Ring.Running, projectRing(a, viewOf(a, tasks = listOf(task(TaskStatus.Running)))))
    }

    @Test
    fun goesByTheViewNotTheFlagForTheProjectOnScreen() {
        val flagged = info("a", true, true)
        assertEquals(Ring.Ready, projectRing(flagged, viewOf(flagged)))
        val idle = info("a")
        assertEquals(Ring.Running, projectRing(idle, viewOf(idle, tasks = listOf(task(TaskStatus.Merging)))))
    }

    @Test
    fun isRunningForAProjectInTheBackgroundWhenTheCoreSaysWorkIsOnInIt() {
        val shown = viewOf(info("b"))
        assertEquals(Ring.Running, projectRing(info("a", true, true), shown))
        assertEquals(Ring.Running, projectRing(info("a", true, true), null))
        assertEquals(Ring.Ready, projectRing(info("a", true, false), shown))
    }

    @Test
    fun neverRunsAProjectThatIsNotOpen() {
        assertEquals(Ring.Ready, projectRing(info("a", open = false, running = true), null))
    }

    @Test
    fun doesNotTakeAnotherProjectsWorkForThisOnes() {
        val shown = viewOf(info("b"), sessions = listOf(session(turnRunning = true)))
        assertEquals(Ring.Ready, projectRing(info("a"), shown))
    }

    // ---- backgroundProjects ----

    private val list = listOf(info("a"), info("b"), info("c", open = false), info("d"))

    @Test
    fun listsTheOpenProjectsOtherThanTheOneOnScreenInOrder() {
        assertEquals(listOf("b", "d"), backgroundProjects(list, "a").map { it.id })
    }

    @Test
    fun listsEveryOpenProjectWhenNoneIsOnScreen() {
        assertEquals(listOf("a", "b", "d"), backgroundProjects(list, null).map { it.id })
    }

    @Test
    fun neverListsAStoppedProjectAndIsEmptyWhenNothingElseRuns() {
        assertEquals(emptyList(), backgroundProjects(listOf(info("a"), info("c", open = false)), "a"))
        assertEquals(emptyList(), backgroundProjects(emptyList(), null))
    }
}
