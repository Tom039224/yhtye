import type { State, StepStatus, Task } from "../api/generated";
import type { ProjectView } from "../store/project";
import { useStore } from "../store/useStore";
import { HELP_LABEL, isTerminal, TONE_LABEL, taskTone } from "./labels";
import {
  formatElapsed,
  isActiveTone,
  openHelps,
  taskAgent,
  taskElapsedMs,
  taskLog,
  taskMeta,
  taskProgress,
} from "./taskInfo";

interface Props {
  task: Task;
  state: State;
  view: ProjectView;
  now: number;
  selected: boolean;
  onSelect: () => void;
  onMention: () => void;
}

const STEP_MARK: Record<StepStatus, string> = { done: "✓", running: "●", pending: "·" };

/**
 * A task card (design §3.5): status badge, id, title (click: agent output),
 * `@` (quote into the composer) and ■ (cancel the task; not on finished
 * tasks), a progress bar while an agent works, the open help as the handling
 * note, the agent's latest activity, the steps, and the agent / elapsed time.
 */
export function TaskCard({ task, state, view, now, selected, onSelect, onMention }: Props) {
  const store = useStore();
  const tone = taskTone(task);
  const helps = openHelps(state, task.id);
  const agent = taskAgent(view, task);
  const log = taskLog(view, task.id);
  const elapsed = taskElapsedMs(view, task, now);
  const meta = taskMeta(state, task, agent);
  const active = isActiveTone(tone);
  return (
    <article className={`task task-card-${tone} ${selected ? "selected" : ""}`} aria-label={`task ${task.id}`}>
      <div className="task-header">
        <span className={`badge tone-${tone}`}>
          {active ? <span className="badge-dot" /> : null}
          {TONE_LABEL[tone]}
        </span>
        <span className="task-id">{task.id}</span>
        <button type="button" className="task-title linklike" onClick={onSelect} title="エージェントの出力を見る">
          {task.title}
        </button>
        <span className="spacer" />
        <div className="task-actions">
          <button type="button" className="icon-btn" title="オーケストレータへ引用" aria-label={`${task.id} をオーケストレータへ引用`} onClick={onMention}>
            @
          </button>
          {!isTerminal(task) ? (
            <button
              type="button"
              className="icon-btn"
              title="エージェントを停止 (タスクを中止)"
              aria-label={`${task.id} を中止`}
              onClick={() => void store.cancelTask(task.id)}
            >
              <span className="stop-square" />
            </button>
          ) : null}
        </div>
      </div>
      {active ? (
        <div className={`progress progress-${tone}`} role="progressbar" aria-valuenow={Math.round(taskProgress(task) * 100)} aria-valuemin={0} aria-valuemax={100}>
          <div className="progress-fill" style={{ width: `${Math.round(taskProgress(task) * 100)}%` }} />
          <div className="progress-sheen" />
        </div>
      ) : null}
      {task.status === "interrupted" ? <div className="task-note">再起動で中断されました。自動で再開します。</div> : null}
      {helps.map((h) => (
        <div key={h.id} className="task-note" role="alert">
          <span className="kind">
            {h.id} · {HELP_LABEL[h.kind]}
            {h.agent_lost ? " · エージェント喪失" : ""}
          </span>
          {h.message}
        </div>
      ))}
      {task.cancel_reason ? <div className="task-meta">中止: {task.cancel_reason}</div> : null}
      {log.length > 0 && !isTerminal(task) ? (
        <div className="task-log" data-testid={`log-${task.id}`}>
          {log.map((l, i) => (
            <div key={i}>{l}</div>
          ))}
        </div>
      ) : null}
      <div className="task-steps" aria-label="steps">
        {task.steps.map((s, i) => (
          <span key={i} className={s.status} title={s.result ?? undefined}>
            {i > 0 ? " → " : ""}
            {s.kind} {STEP_MARK[s.status]}
            {s.verdict ? ` (${s.verdict})` : ""}
          </span>
        ))}
      </div>
      {meta ? <div className="task-meta">{meta}</div> : null}
      <div className="task-footer">
        {agent ?? "エージェント未起動"}
        {elapsed !== null ? ` · ${formatElapsed(elapsed)}` : ""}
      </div>
    </article>
  );
}
