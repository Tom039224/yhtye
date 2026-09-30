// The domain reducer, a TypeScript port of `State::apply`
// (crates/yhtye-core/src/domain/event.rs). Pure and immutable. Its agreement
// with the Rust reducer is checked in domain.test.ts by replaying a stream
// recorded from the real core (src/test/fixtures/fake-run.json).

import type {
  DomainEvent,
  Group,
  Help,
  State,
  Step,
  StepSpec,
  Task,
} from "../api/generated";

function stepFromSpec(spec: StepSpec): Step {
  return {
    kind: spec.kind,
    instruction: spec.instruction ?? null,
    status: "pending",
    result: null,
    verdict: null,
    note: null,
    nudges: 0,
  };
}

function mapGroup(state: State, id: string, f: (g: Group) => Group): State {
  return { ...state, groups: state.groups.map((g) => (g.id === id ? f(g) : g)) };
}

function mapHelp(state: State, id: string, f: (h: Help) => Help): State {
  return { ...state, helps: state.helps.map((h) => (h.id === id ? f(h) : h)) };
}

function mapTask(state: State, id: string, f: (t: Task) => Task): State {
  return { ...state, tasks: state.tasks.map((t) => (t.id === id ? f(t) : t)) };
}

function mapStep(state: State, task: string, index: number, f: (s: Step) => Step): State {
  return mapTask(state, task, (t) =>
    index < t.steps.length ? { ...t, steps: t.steps.map((s, i) => (i === index ? f(s) : s)) } : t,
  );
}

function unreachable(event: never): never {
  throw new Error(`unknown domain event ${JSON.stringify(event)}`);
}

/** Applies one event. Like the Rust reducer, unknown ids change nothing. */
export function applyDomainEvent(state: State, event: DomainEvent): State {
  switch (event.type) {
    case "group_created":
      return {
        ...state,
        counters: { ...state.counters, groups: state.counters.groups + 1 },
        groups: [...state.groups, event.group],
      };
    case "group_base_changed":
      // `finish_group` with `into` (Stage 8e): the group merges into another branch.
      return mapGroup(state, event.group, (g) => ({ ...g, base_branch: event.to }));
    case "group_finishing":
      return mapGroup(state, event.group, (g) => ({
        ...g,
        status: "finishing",
        finish_summary: event.summary,
      }));
    case "group_finish_reminded":
      // `?? 0`: groups created before Stage 7a have no counter in their event.
      return mapGroup(state, event.group, (g) => ({ ...g, finish_nudges: (g.finish_nudges ?? 0) + 1 }));
    case "group_merge_finished":
      return mapGroup(state, event.group, (g) => ({
        ...g,
        status: event.ok ? "done" : "merge_blocked",
        detail: event.detail,
      }));
    case "group_cancelled":
      return mapGroup(state, event.group, (g) => ({ ...g, status: "cancelled", detail: event.reason }));
    case "help_raised":
      return {
        ...state,
        counters: { ...state.counters, helps: state.counters.helps + 1 },
        helps: [...state.helps, event.help],
      };
    case "help_answered": {
      const help = state.helps.find((h) => h.id === event.help);
      if (!help) return state;
      const answered = mapHelp(state, event.help, (h) => ({
        ...h,
        state: "answered",
        reply: event.reply,
      }));
      // The answer is a new prompt: the agent gets a fresh reminder budget.
      return mapStep(answered, help.task, help.step, (s) => ({ ...s, nudges: 0 }));
    }
    case "help_closed":
      return mapHelp(state, event.help, (h) => ({ ...h, state: "closed" }));
    case "help_agent_lost":
      return mapHelp(state, event.help, (h) => ({ ...h, agent_lost: true }));
    case "inbox_queued":
      return {
        ...state,
        counters: { ...state.counters, inbox: Math.max(state.counters.inbox, event.entry.id) },
        inbox: [...state.inbox, event.entry],
      };
    case "inbox_delivered":
      return { ...state, inbox: state.inbox.filter((e) => e.chat !== event.chat || e.id > event.up_to) };
    case "chat_created":
      return {
        ...state,
        counters: { ...state.counters, chats: state.counters.chats + 1 },
        chats: [...state.chats, event.chat],
      };
    case "chat_titled":
      return { ...state, chats: state.chats.map((c) => (c.id === event.chat ? { ...c, title: event.title } : c)) };
    case "task_created":
      return {
        ...state,
        counters: { ...state.counters, tasks: state.counters.tasks + 1 },
        tasks: [...state.tasks, event.task],
      };
    default:
      return applyTaskEvent(state, event);
  }
}

type TaskEvent = Exclude<
  DomainEvent,
  {
    type:
      | "group_created"
      | "group_base_changed"
      | "group_finishing"
      | "group_finish_reminded"
      | "group_merge_finished"
      | "group_cancelled"
      | "help_raised"
      | "help_answered"
      | "help_closed"
      | "help_agent_lost"
      | "inbox_queued"
      | "inbox_delivered"
      | "chat_created"
      | "chat_titled"
      | "task_created";
  }
>;

function applyTaskEvent(state: State, event: TaskEvent): State {
  switch (event.type) {
    case "instruction_set":
      return mapTask(state, event.task, (t) => ({ ...t, instruction: event.instruction }));
    case "task_status_changed":
      return mapTask(state, event.task, (t) => ({ ...t, status: event.status }));
    case "task_cancelled":
      return mapTask(state, event.task, (t) => ({
        ...t,
        status: "cancelled",
        cancel_reason: event.reason,
      }));
    case "workspace_ready":
      return mapTask(state, event.task, (t) => ({ ...t, workdir: event.path }));
    case "steps_replaced":
      return mapTask(state, event.task, (t) => ({
        ...t,
        steps: [...t.steps.slice(0, event.from), ...event.steps.map(stepFromSpec)],
      }));
    case "review_steps_inserted":
      return mapTask(state, event.task, (t) => {
        const at = Math.min(event.after + 1, t.steps.length);
        const steps = [...t.steps.slice(0, at), ...event.steps.map(stepFromSpec), ...t.steps.slice(at)];
        return { ...t, steps, review_rounds: t.review_rounds + 1 };
      });
    case "step_started":
      return mapTask(state, event.task, (t) => ({
        ...t,
        current: event.step,
        steps: t.steps.map((s, i) =>
          i === event.step ? { ...s, status: "running", note: event.note, nudges: 0 } : s,
        ),
      }));
    case "step_completed":
      return mapStep(state, event.task, event.step, (s) => ({
        ...s,
        status: "done",
        result: event.result,
        verdict: event.verdict,
      }));
    case "step_reset":
      return mapStep(state, event.task, event.step, (s) => ({ ...s, status: "pending" }));
    case "nudge_sent":
      return mapStep(state, event.task, event.step, (s) => ({ ...s, nudges: s.nudges + 1 }));
    default:
      return unreachable(event);
  }
}

/** An empty state for `project` (what the core starts a new project with). */
export function emptyState(project: string, config: State["config"]): State {
  return {
    project,
    config,
    chats: [],
    groups: [],
    tasks: [],
    helps: [],
    inbox: [],
    counters: { chats: 0, groups: 0, tasks: 0, helps: 0, inbox: 0 },
  };
}
