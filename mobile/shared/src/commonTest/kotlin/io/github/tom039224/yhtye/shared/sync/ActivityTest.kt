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
    private fun info(id: String, open: Boolean = true, running: Boolean = false, awake: Boolean = false) =
        ProjectInfo(id, id, "/work/$id", open, running, awake)

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

    // ---- hasLiveOrchestrator ----

    @Test
    fun isTrueWhileOneOfTheChatsHasAnOrchestratorSessionUp() {
        assertTrue(hasLiveOrchestrator(viewOf(info("a"), sessions = listOf(session()))))
        assertFalse(hasLiveOrchestrator(viewOf(info("a"), sessions = listOf(session(SessionStatus.Stopped)))))
        assertFalse(hasLiveOrchestrator(viewOf(info("a"))))
    }

    @Test
    fun ignoresTheSessionsOfTasks() {
        assertFalse(hasLiveOrchestrator(viewOf(info("a"), sessions = listOf(sessionRecord("T-1/implementer")))))
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
    fun takesOpenRunningAndAwakeOfTheProjectsItKnowsKeepingTheRest() {
        val known = listOf(info("a").copy(name = "A"), info("b", open = false))
        val merged = mergeActivity(known, listOf(info("a", true, true, true), info("b", true, false), info("c", true, true)))
        assertEquals(listOf(info("a", true, true, true).copy(name = "A"), info("b", true, false)), merged)
    }

    @Test
    fun noticesOnlyAwakeChangingToo() {
        val known = listOf(info("a", true, false, false))
        assertEquals(listOf(info("a", true, false, true)), mergeActivity(known, listOf(info("a", true, false, true))))
    }

    @Test
    fun isTheSameListWhenNothingChangedAndNeverDropsAProjectTheAnswerLacks() {
        val known = listOf(info("a"), info("b", true, true))
        assertSame(known, mergeActivity(known, listOf(info("b", true, true), info("a"))))
        assertSame(known, mergeActivity(known, emptyList()))
    }

    // ---- rememberRunning ----

    @Test
    fun setsTheFlagsOfTheProjectLeftFromWhatItsViewShowed() {
        val a = info("a")
        val busy = viewOf(a, tasks = listOf(task(TaskStatus.Running)))
        assertEquals(listOf(a.copy(running = true), info("b")), rememberRunning(listOf(a, info("b")), busy))
        val idle = viewOf(info("a", true, true))
        assertEquals(listOf(info("a", true, false)), rememberRunning(listOf(info("a", true, true)), idle))
    }

    @Test
    fun remembersAnAwakeProjectLeftWhileItsOrchestratorIsUp() {
        val projects = listOf(info("a", true, false, false))
        val left = viewOf(info("a"), sessions = listOf(session()))
        assertEquals(listOf(info("a", true, false, true)), rememberRunning(projects, left))
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
    fun isStoppedForAProjectThatIsNotOpen() {
        assertEquals(Ring.Stopped, projectRing(info("a", open = false), null))
        assertEquals(Ring.Stopped, projectRing(info("a", open = false), viewOf(info("b"))))
        assertEquals(Ring.Stopped, projectRing(info("a", open = false, running = true), null))
    }

    @Test
    fun isStoppedForAnOpenProjectWhoseChatsAllSleep() {
        assertEquals(Ring.Stopped, projectRing(info("a"), null))
        assertEquals(Ring.Stopped, projectRing(info("a"), viewOf(info("a"))))
        assertEquals(Ring.Stopped, projectRing(info("a"), viewOf(info("b"))))
    }

    @Test
    fun isReadyForAnOpenProjectWithAnOrchestratorUpButIdle() {
        assertEquals(Ring.Ready, projectRing(info("a", true, false, true), null))
        assertEquals(Ring.Ready, projectRing(info("a", true, false, true), viewOf(info("b"))))
        assertEquals(Ring.Ready, projectRing(info("a"), viewOf(info("a"), sessions = listOf(session()))))
    }

    @Test
    fun isStoppedWithoutALiveOrchestratorSessionEvenWithATaskAgentUp() {
        val view = viewOf(info("a"), sessions = listOf(sessionRecord("T-1/implementer")))
        assertEquals(Ring.Stopped, projectRing(info("a"), view))
    }

    @Test
    fun isRunningForTheProjectOnScreenWhileWorkIsOnInItsView() {
        val a = info("a")
        assertEquals(Ring.Running, projectRing(a, viewOf(a, sessions = listOf(session(turnRunning = true)))))
        assertEquals(Ring.Running, projectRing(a, viewOf(a, tasks = listOf(task(TaskStatus.Running)))))
    }

    @Test
    fun goesByTheViewNotTheFlagsForTheProjectOnScreen() {
        val flagged = info("a", true, true, true)
        assertEquals(Ring.Stopped, projectRing(flagged, viewOf(flagged)))
        val idle = info("a")
        assertEquals(Ring.Running, projectRing(idle, viewOf(idle, tasks = listOf(task(TaskStatus.Merging)))))
        val sleeping = info("a", true, false, true)
        assertEquals(Ring.Stopped, projectRing(sleeping, viewOf(sleeping)))
    }

    @Test
    fun isRunningForAProjectInTheBackgroundWhenTheCoreSaysWorkIsOnInIt() {
        val shown = viewOf(info("b"))
        assertEquals(Ring.Running, projectRing(info("a", true, true), shown))
        assertEquals(Ring.Running, projectRing(info("a", true, true), null))
        assertEquals(Ring.Ready, projectRing(info("a", true, false, true), shown))
        assertEquals(Ring.Stopped, projectRing(info("a", true, false, false), shown))
    }

    @Test
    fun doesNotTakeAnotherProjectsWorkForThisOnes() {
        val shown = viewOf(info("b"), sessions = listOf(session(turnRunning = true)))
        assertEquals(Ring.Stopped, projectRing(info("a"), shown))
        assertEquals(Ring.Ready, projectRing(info("a", true, false, true), shown))
    }

    // ---- backgroundProjects ----

    // a: 起きている、b: 作業中、c: 開いていない、d: 起きている、e: 開いているがチャットはみな眠っている
    private val list = listOf(info("a", awake = true), info("b", running = true), info("c", open = false), info("d", awake = true), info("e"))

    @Test
    fun listsTheOpenProjectsWithSomethingUpOtherThanTheOneOnScreenInOrder() {
        assertEquals(listOf("b", "d"), backgroundProjects(list, "a").map { it.id })
    }

    @Test
    fun listsEverySuchProjectWhenNoneIsOnScreen() {
        assertEquals(listOf("a", "b", "d"), backgroundProjects(list, null).map { it.id })
    }

    @Test
    fun leavesOutAProjectWhoseChatsAllSleepAndIsEmptyWhenNothingElseRuns() {
        assertEquals(emptyList(), backgroundProjects(listOf(info("e")), null))
        assertEquals(emptyList(), backgroundProjects(listOf(info("a"), info("c", open = false)), "a"))
        assertEquals(emptyList(), backgroundProjects(emptyList(), null))
    }

    @Test
    fun dropsAProjectOnceItWasPutToSleep() {
        val awake = listOf(info("a", awake = true), info("b", awake = true))
        assertEquals(listOf("b"), backgroundProjects(awake, "a").map { it.id })
        val slept = awake.map { if (it.id == "b") it.copy(awake = false) else it }
        assertEquals(emptyList(), backgroundProjects(slept, "a"))
    }

    // ---- canSleepProject ----

    @Test
    fun canSleepAnOpenBackgroundProjectWithAnIdleOrchestratorUp() {
        assertTrue(canSleepProject(info("a", awake = true), null))
        assertTrue(canSleepProject(info("a", awake = true), viewOf(info("b"))))
    }

    @Test
    fun cannotSleepAProjectThatIsNotOpenOrWhoseChatsAllSleep() {
        assertFalse(canSleepProject(info("a", open = false), null))
        assertFalse(canSleepProject(info("a"), viewOf(info("b"))))
    }

    @Test
    fun cannotSleepAProjectWhileWorkIsOn() {
        assertFalse(canSleepProject(info("a", running = true, awake = true), viewOf(info("b"))))
        val a = info("a")
        assertFalse(canSleepProject(a, viewOf(a, sessions = listOf(session(turnRunning = true)))))
        assertFalse(canSleepProject(a, viewOf(a, tasks = listOf(task(TaskStatus.Running)))))
    }

    @Test
    fun judgesTheProjectOnScreenByItsView() {
        val a = info("a")
        assertTrue(canSleepProject(a, viewOf(a, sessions = listOf(session()))))
        assertFalse(canSleepProject(a, viewOf(a)))
    }
}
