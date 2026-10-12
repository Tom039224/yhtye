package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.Chat
import io.github.tom039224.yhtye.shared.api.generated.Counters
import io.github.tom039224.yhtye.shared.api.generated.DomainConfig
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.Group
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Help
import io.github.tom039224.yhtye.shared.api.generated.HelpAction
import io.github.tom039224.yhtye.shared.api.generated.HelpKind
import io.github.tom039224.yhtye.shared.api.generated.HelpSource
import io.github.tom039224.yhtye.shared.api.generated.HelpState
import io.github.tom039224.yhtye.shared.api.generated.InboxEntry
import io.github.tom039224.yhtye.shared.api.generated.InboxItem
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.Step
import io.github.tom039224.yhtye.shared.api.generated.StepKind
import io.github.tom039224.yhtye.shared.api.generated.StepSpec
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskKind
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** PC の `src/store/domain.test.ts` の移植: Kotlin の reducer がコアと同じ結果になる。 */
class DomainTest {
    private fun domainEvents(r: Recording): List<DomainEvent> =
        durable(r).mapNotNull { (it.body as? ApiEventBody.Domain)?.event }

    private val recordings = listOf(
        "run to merge" to { FULL_RUN },
        "cancellations" to { CANCEL_RUN },
        "group left open, finished by Yhtye" to { UNFINISHED_RUN },
    )

    @Test
    fun replaysTheWholeLogOfEachRecordingToTheCoresFinalState() {
        for ((name, rec) in recordings) {
            val r = rec()
            val empty = emptyState(r.end.state.project, r.end.state.config)
            assertEquals(r.end.state, domainEvents(r).fold(empty, ::applyDomainEvent), name)
        }
    }

    @Test
    fun foldsEachRecordingFromTheStartSnapshotToTheFinalStateAndSessions() {
        for ((name, rec) in recordings) {
            val r = rec()
            val events = durable(r).filter { it.seq > r.start.seq }
            val state = events.fold(r.start.state) { s, e -> (e.body as? ApiEventBody.Domain)?.let { applyDomainEvent(s, it.event) } ?: s }
            val sessions = events.fold(r.start.sessions) { s, e -> applySessionEvent(s, e.body) }
            assertEquals(r.end.state, state, name)
            assertEquals(r.end.sessions, sessions, name)
        }
    }

    @Test
    fun doesNotChangeTheInputState() {
        val empty = emptyState("p", FULL_RUN.end.state.config)
        domainEvents(FULL_RUN).fold(empty, ::applyDomainEvent)
        assertEquals(emptyList(), empty.tasks)
    }

    // ---- 記録に無い遷移 (Rust の reducer の規則と照らす) ----

    private val task = Task(
        id = "T-1",
        group = "G-1",
        title = "t",
        kind = TaskKind.Code,
        dependsOn = emptyList(),
        instruction = null,
        steps = listOf(
            Step(StepKind.Implement, null, StepStatus.Done, "r", null, null, 0),
            Step(StepKind.Review, null, StepStatus.Running, null, null, null, 1),
            Step(StepKind.Done, null, StepStatus.Pending, null, null, null, 0),
        ),
        current = 1,
        status = TaskStatus.Running,
        reviewRounds = 0,
    )
    private val base: State = emptyState("p", DomainConfig(2)).copy(tasks = listOf(task))
    private val help = Help(
        id = "H-1",
        task = "T-1",
        step = 1,
        kind = HelpKind.Blocked,
        message = "stuck",
        source = HelpSource.Agent(Role.Reviewer),
        state = HelpState.Open,
        agentLost = false,
    )

    private fun group(id: String, status: GroupStatus = GroupStatus.Active, base: String = "main") =
        Group(id, "C-1", "g", null, base, "yhtye/$id", status)

    @Test
    fun insertsReviewStepsAfterTheReviewAndCountsTheRound() {
        val s = applyDomainEvent(
            base,
            DomainEvent.ReviewStepsInserted("T-1", 1, listOf(StepSpec(StepKind.Implement, "fix"), StepSpec(StepKind.Review))),
        )
        assertEquals(
            listOf(StepKind.Implement, StepKind.Review, StepKind.Implement, StepKind.Review, StepKind.Done),
            s.tasks[0].steps.map { it.kind },
        )
        assertEquals("fix", s.tasks[0].steps[2].instruction)
        assertNull(s.tasks[0].steps[3].instruction)
        assertEquals(1, s.tasks[0].reviewRounds)
    }

    @Test
    fun replacesStepsFromAnIndex() {
        val s = applyDomainEvent(base, DomainEvent.StepsReplaced("T-1", 2, listOf(StepSpec(StepKind.Checkpoint), StepSpec(StepKind.Done))))
        assertEquals(listOf(StepKind.Implement, StepKind.Review, StepKind.Checkpoint, StepKind.Done), s.tasks[0].steps.map { it.kind })
    }

    @Test
    fun raisesAnswersClosesAndLosesHelps() {
        var s = applyDomainEvent(base, DomainEvent.HelpRaised(help))
        assertEquals(1, s.counters.helps)
        s = applyDomainEvent(s, DomainEvent.HelpAgentLost("H-1"))
        assertTrue(s.helps[0].agentLost)
        s = applyDomainEvent(s, DomainEvent.HelpAnswered("H-1", HelpAction.Resume, "go on"))
        assertEquals(HelpState.Answered to "go on", s.helps[0].state to s.helps[0].reply)
        // 答えは新しいプロンプト: ステップの催促は 0 に戻る
        assertEquals(0, s.tasks[0].steps[1].nudges)
        s = applyDomainEvent(s, DomainEvent.HelpClosed("H-1"))
        assertEquals(HelpState.Closed, s.helps[0].state)
    }

    @Test
    fun countsNudgesResetsStepsAndSetsInstructionsAndWorkdirs() {
        var s = applyDomainEvent(base, DomainEvent.NudgeSent("T-1", 1))
        assertEquals(2, s.tasks[0].steps[1].nudges)
        s = applyDomainEvent(s, DomainEvent.StepReset("T-1", 0))
        assertEquals(StepStatus.Pending, s.tasks[0].steps[0].status)
        s = applyDomainEvent(s, DomainEvent.InstructionSet("T-1", "do it"))
        s = applyDomainEvent(s, DomainEvent.WorkspaceReady("T-1", "/w"))
        assertEquals("do it" to "/w", s.tasks[0].instruction to s.tasks[0].workdir)
    }

    @Test
    fun marksAFailedGroupMergeAsMergeBlocked() {
        var s = applyDomainEvent(base, DomainEvent.GroupCreated(group("G-1")))
        s = applyDomainEvent(s, DomainEvent.GroupFinishing("G-1", "sum"))
        s = applyDomainEvent(s, DomainEvent.GroupMergeFinished("G-1", ok = false, detail = "dirty"))
        val g = s.groups[0]
        assertEquals(Triple(GroupStatus.MergeBlocked, "dirty", "sum"), Triple(g.status, g.detail, g.finishSummary))
    }

    @Test
    fun countsFinishReminders() {
        // 記録より前のグループには数が無い (生成された型の既定値 0 で読む)
        var s = applyDomainEvent(base, DomainEvent.GroupCreated(group("G-1")))
        s = applyDomainEvent(s, DomainEvent.GroupFinishReminded("G-1"))
        s = applyDomainEvent(s, DomainEvent.GroupFinishReminded("G-1"))
        assertEquals(2, s.groups[0].finishNudges)
    }

    @Test
    fun keepsChatsBoundToTheirWorktreeAndMovesOnlyTheGroupFinishGroupIntoNames() {
        var s = applyDomainEvent(base, DomainEvent.ChatCreated(Chat("C-1", "/wt/feature", null)))
        s = applyDomainEvent(s, DomainEvent.GroupCreated(group("G-1", GroupStatus.MergeBlocked, "feature")))
        s = applyDomainEvent(s, DomainEvent.GroupCreated(group("G-2", GroupStatus.MergeBlocked, "feature")))
        s = applyDomainEvent(s, DomainEvent.GroupBaseChanged("G-2", "feature", "renamed"))
        assertEquals(listOf(Chat("C-1", "/wt/feature", null)), s.chats)
        assertEquals(listOf("G-1" to "feature", "G-2" to "renamed"), s.groups.map { it.id to it.baseBranch })
    }

    @Test
    fun deletesAChatWithItsGroupsTheirTasksAndHelpsAndItsInboxAndKeepsTheRest() {
        val h = help.copy(step = 0, source = HelpSource.Agent(Role.Implementer))
        val entry = { id: Long, chat: String -> InboxEntry(id, chat, InboxItem(InboxKind.UserMessage, emptyList(), "hi")) }
        val state = chatState().copy(
            helps = listOf(h),
            inbox = listOf(entry(1, "C-1"), entry(2, "C-2")),
            counters = Counters(chats = 2, groups = 2, tasks = 2, helps = 1, inbox = 2),
        )
        val after = applyDomainEvent(state, DomainEvent.ChatDeleted("C-1"))
        assertEquals(listOf("C-2"), after.chats.map { it.id })
        assertEquals(listOf("G-2"), after.groups.map { it.id })
        assertEquals(listOf("T-2"), after.tasks.map { it.id })
        assertEquals(emptyList(), after.helps)
        assertEquals(listOf("C-2"), after.inbox.map { it.chat })
        assertEquals(state.counters, after.counters)
        assertEquals(after, applyDomainEvent(after, DomainEvent.ChatDeleted("C-1")))
    }

    @Test
    fun ignoresEventsForUnknownIds() {
        assertEquals(base, applyDomainEvent(base, DomainEvent.TaskStatusChanged("T-9", TaskStatus.Done)))
        assertEquals(base, applyDomainEvent(base, DomainEvent.StepCompleted("T-1", 7, "x", null)))
    }

    @Test
    fun leavesTheStateAloneForAnEventFromANewerCore() {
        // Kotlin だけ: 知らないイベント (新しい PC) は状態を変えない
        val unknown: DomainEvent = DomainEvent.Unrecognized("something_new", JsonObject(emptyMap()))
        assertSame(base, applyDomainEvent(base, unknown))
        assertTrue(unknown is UnrecognizedVariant)
    }
}
