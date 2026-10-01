import { Fragment } from "react";

import type { State, Task } from "../api/generated";
import type { ProjectView } from "../store/project";
import { useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";
import { HELP_LABEL, isTerminal, TONE_LABEL, taskTone } from "./labels";
import {
  HELP_ICON,
  KIND_LABEL,
  STEP_LABEL,
  STEP_STATUS_LABEL,
  StatusChip,
  stepIcon,
  ToolStatus,
  toneIcon,
} from "./statusIcons";
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

const LOG_ICON = { message: "message", tool: "wrench", error: "alert" } as const;

/**
 * A task card (design §3.5): the status as an icon chip, id, title (click:
 * agent output), icon buttons to quote it into the composer and to cancel it
 * (not on finished tasks), a progress bar while an agent works, the open help
 * as the handling note, the agent's latest activity, the steps as a row of
 * icons, and the agent / elapsed time.
 */
export function TaskCard({ task, state, view, now, selected, onSelect, onMention }: Props) {
  const store = useStore();
  const tone = taskTone(task);
  const helps = openHelps(state, task.id);
  const agent = taskAgent(view, task);
  const log = taskLog(view, task.id);
  const elapsed = taskElapsedMs(view, task, now);
  const meta = taskMeta(state, task);
  const active = isActiveTone(tone);
  const percent = Math.round(taskProgress(task) * 100);
  return (
    <article className={`task task-card-${tone} ${selected ? "selected" : ""}`} aria-label={`task ${task.id}`}>
      <div className="task-header">
        <StatusChip className={`tone-${tone}`} icon={toneIcon(tone, task.kind)} label={TONE_LABEL[tone]} active={active} />
        <span className="task-id" title={KIND_LABEL[task.kind]}>
          {task.id}
        </span>
        <button type="button" className="task-title linklike" onClick={onSelect} title="エージェントの出力を見る">
          {task.title}
        </button>
        <span className="spacer" />
        <div className="task-actions">
          <IconButton icon="at" title="オーケストレータへ引用" label={`${task.id} をオーケストレータへ引用`} onClick={onMention} />
          {!isTerminal(task) ? (
            <IconButton
              icon="stop"
              title="エージェントを停止 (タスクを中止)"
              label={`${task.id} を中止`}
              onClick={() => void store.cancelTask(task.id)}
            />
          ) : null}
        </div>
      </div>
      {active ? (
        <div className={`progress progress-${tone}`} role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
          <div className="progress-fill" style={{ width: `${percent}%` }} />
          <div className="progress-sheen" />
        </div>
      ) : null}
      {task.status === "interrupted" ? (
        <div className="task-note">
          <Icon name="refresh" size={13} />
          再起動で中断されました。自動で再開します。
        </div>
      ) : null}
      {helps.map((h) => (
        <div key={h.id} className="task-note" role="alert">
          <span className="help-kind" title={`${h.id} · ${HELP_LABEL[h.kind]}`}>
            <Icon name={HELP_ICON[h.kind]} size={13} />
            <span className="sr-only">{HELP_LABEL[h.kind]}</span>
          </span>
          {h.agent_lost ? (
            <span className="help-kind" title="エージェントの処理が失われました">
              <Icon name="plug-off" size={13} />
              <span className="sr-only">エージェント喪失</span>
            </span>
          ) : null}
          {h.message}
        </div>
      ))}
      {task.cancel_reason ? (
        <div className="task-meta" title="中止した理由">
          <Icon name="ban" size={12} />
          {task.cancel_reason}
        </div>
      ) : null}
      {log.length > 0 && !isTerminal(task) ? (
        <div className="task-log" data-testid={`log-${task.id}`}>
          {log.map((l, i) => (
            <div key={i} className={`log-line log-${l.kind}`}>
              <Icon name={LOG_ICON[l.kind]} size={11} />
              <span className="log-text">{l.text}</span>
              {l.kind === "tool" ? <ToolStatus status={l.status ?? null} /> : null}
            </div>
          ))}
        </div>
      ) : null}
      <div className="task-foot">
        <ol className="steps" aria-label="steps">
          {task.steps.map((s, i) => {
            const label = `${STEP_LABEL[s.kind]} · ${STEP_STATUS_LABEL[s.status]}`;
            const running = s.status === "running";
            return (
              <Fragment key={i}>
                {i > 0 ? <li className="step-link" aria-hidden="true" /> : null}
                <li
                  className={`step is-${s.status} kind-${s.kind} ${s.verdict ? `verdict-${s.verdict}` : ""}`}
                  aria-label={`${s.kind} ${s.status}${s.verdict ? ` (${s.verdict})` : ""}`}
                  title={[label, s.verdict ? verdictLabel(s.verdict) : null, s.result].filter(Boolean).join("\n")}
                >
                  <Icon name={stepIcon(s.kind, task.kind)} size={13} className={running ? "step-pulse" : ""} />
                </li>
              </Fragment>
            );
          })}
        </ol>
        <span className="spacer" />
        {agent ? (
          <span className="task-agent" title="エージェントのセッション · 作業時間">
            <Icon name="bot" size={12} />
            {agent}
            {elapsed !== null ? ` · ${formatElapsed(elapsed)}` : ""}
          </span>
        ) : null}
      </div>
      {meta ? (
        <div className="task-meta" title={meta.title}>
          <Icon name={meta.icon} size={12} />
          {meta.text}
        </div>
      ) : null}
    </article>
  );
}

function verdictLabel(verdict: string): string {
  return verdict === "approve" ? "承認" : "要修正";
}
