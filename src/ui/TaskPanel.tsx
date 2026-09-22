import type { Group, Help, State, Task } from "../api/generated";
import { useStore } from "../store/useStore";
import { GROUP_LABEL, HELP_LABEL, isTerminal, TONE_LABEL, taskTone } from "./labels";

interface Props {
  state: State;
  selectedTask: string | null;
  onSelectTask: (task: string) => void;
}

export function TaskPanel({ state, selectedTask, onSelectTask }: Props) {
  if (state.groups.length === 0) {
    return <p className="empty">グループはまだありません。オーケストレータがタスクを作るとここに出ます。</p>;
  }
  // Newest group first: the active one is what the user watches.
  const groups = [...state.groups].reverse();
  return (
    <div className="groups">
      {groups.map((g) => (
        <GroupCard key={g.id} group={g} state={state} selectedTask={selectedTask} onSelectTask={onSelectTask} />
      ))}
    </div>
  );
}

function GroupCard({ group, state, selectedTask, onSelectTask }: { group: Group } & Props) {
  const store = useStore();
  const tasks = state.tasks.filter((t) => t.group === group.id);
  const done = tasks.filter((t) => t.status === "done").length;
  const handling = tasks.filter((t) => taskTone(t) === "handling").length;
  const cancellable = group.status === "active" || group.status === "merge_blocked";
  return (
    <section className={`group group-${group.status}`} aria-label={`group ${group.id}`}>
      <header className="group-header">
        <span className="mono dim">{group.id}</span>
        <span className="group-title">{group.title}</span>
        <span className={`badge mono group-status-${group.status}`}>{GROUP_LABEL[group.status]}</span>
        <span className="mono dim">
          {done}/{tasks.length} 完了{handling > 0 ? ` · ${handling} 件 対処中` : ""}
        </span>
        {cancellable ? (
          <button type="button" className="btn btn-small" onClick={() => void store.cancelGroup(group.id)}>
            グループを中止
          </button>
        ) : null}
      </header>
      <div className="mono dim branch">
        {group.group_branch} ← {group.base_branch}
      </div>
      {group.status === "merge_blocked" ? (
        <div className="alert alert-handling" role="alert">
          base ブランチへのマージが保留されています: {group.detail ?? "理由不明"}
        </div>
      ) : null}
      {group.detail && group.status !== "merge_blocked" ? <div className="mono dim">{group.detail}</div> : null}
      {tasks.length === 0 ? <p className="empty">タスクはまだありません。</p> : null}
      {tasks.map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          helps={state.helps.filter((h) => h.task === t.id && h.state === "open")}
          selected={selectedTask === t.id}
          onSelect={() => onSelectTask(t.id)}
        />
      ))}
    </section>
  );
}

function TaskCard({ task, helps, selected, onSelect }: { task: Task; helps: Help[]; selected: boolean; onSelect: () => void }) {
  const store = useStore();
  const tone = taskTone(task);
  return (
    <article className={`task tone-card-${tone} ${selected ? "task-selected" : ""}`} aria-label={`task ${task.id}`}>
      <div className="task-header">
        <span className={`badge tone-${tone}`}>{TONE_LABEL[tone]}</span>
        <span className="mono">{task.id}</span>
        <button type="button" className="task-title linklike" onClick={onSelect}>
          {task.title}
        </button>
        <span className="mono dim">{task.kind} · {task.status}</span>
        {!isTerminal(task) ? (
          <button type="button" className="btn btn-small" title="タスクを中止" onClick={() => void store.cancelTask(task.id)}>
            ■ 中止
          </button>
        ) : null}
      </div>
      {task.status === "interrupted" ? (
        <div className="alert alert-handling">再起動で中断されました。自動で再開します。</div>
      ) : null}
      {task.status === "awaiting_instruction" ? (
        <div className="mono dim">オーケストレータの指示待ち</div>
      ) : null}
      {helps.map((h) => (
        <div key={h.id} className="alert alert-handling" role="alert">
          <span className="mono">help {h.id} · {HELP_LABEL[h.kind]}</span>
          {h.agent_lost ? <span className="mono"> · エージェント喪失</span> : null}
          <div>{h.message}</div>
        </div>
      ))}
      {task.cancel_reason ? <div className="mono dim">中止: {task.cancel_reason}</div> : null}
      <ol className="steps">
        {task.steps.map((s, i) => (
          <li key={i} className={`step step-${s.status}`}>
            <span className="mono">{s.kind}</span>
            <span className="mono dim"> · {s.status}</span>
            {s.verdict ? <span className="mono"> · {s.verdict}</span> : null}
            {s.result ? <div className="step-result">{s.result}</div> : null}
          </li>
        ))}
      </ol>
    </article>
  );
}
