import { useMemo } from "react";

import type { Group, State } from "../api/generated";
import { scopeState } from "../store/chats";
import type { ProjectView } from "../store/project";
import { useStore } from "../store/useStore";
import type { Mention } from "./mentions";
import { TaskCard } from "./TaskCard";
import { focusGroup, groupLabel, groupProgress, liveSubagents } from "./taskInfo";
import { useNow } from "./useNow";

interface Props {
  view: ProjectView;
  selectedTask: string | null;
  onSelectTask: (task: string) => void;
  onMention: (m: Mention) => void;
}

/**
 * Header, the focus group's task cards, and earlier groups folded (design
 * §3.5). Only the selected chat's groups are shown (orchestrator-desktop §9).
 */
export function TaskColumn({ view, selectedTask, onSelectTask, onMention }: Props) {
  const { state: all, selectedChat } = view;
  const state = useMemo(() => (all ? scopeState(all, selectedChat) : null), [all, selectedChat]);
  const group = state ? focusGroup(state) : null;
  const progress = state && group ? groupProgress(state, group.id) : null;
  const now = useNow(30_000);
  const older = state && group ? [...state.groups].filter((g) => g.id !== group.id).reverse() : [];

  const cards = (g: Group, s: State) =>
    s.tasks
      .filter((t) => t.group === g.id)
      .map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          state={s}
          view={view}
          now={now}
          selected={selectedTask === t.id}
          onSelect={() => onSelectTask(t.id)}
          onMention={() => onMention({ task: t.id, title: t.title })}
        />
      ));

  return (
    <section className="tasks" aria-label="tasks">
      <header className="panel-header">
        <span className="panel-title">tasks</span>
        {group ? <span className="chip" title={group.id}>{group.title}</span> : null}
        {progress ? <span className="panel-meta">{progress.done}/{progress.total} 完了</span> : null}
        <span className="spacer" />
        <span className="panel-meta" title="動いているサブエージェントのセッション数">
          subagents {state ? liveSubagents(view, state) : 0}
        </span>
      </header>
      <div className="scroll">
        {view.phase === "loading" && !state ? <p className="empty">読み込み中…</p> : null}
        {view.phase === "error" ? <p className="empty error-text">{view.loadError}</p> : null}
        {state && !group ? (
          <p className="empty">
            {selectedChat
              ? "グループはまだありません。オーケストレータがタスクを作るとここに出ます。"
              : "チャットを選ぶと、そのチャットのグループがここに出ます。"}
          </p>
        ) : null}
        {state && group ? (
          <div className="task-list">
            <GroupBar group={group} state={state} />
            {cards(group, state)}
            {state.tasks.every((t) => t.group !== group.id) ? <p className="empty">タスクはまだありません。</p> : null}
            {older.length > 0 ? (
              <details className="past-groups">
                <summary>EARLIER GROUPS ({older.length})</summary>
                {older.map((g) => (
                  <div key={g.id} className="past-group">
                    <GroupBar group={g} state={state} />
                    {cards(g, state)}
                  </div>
                ))}
              </details>
            ) : null}
          </div>
        ) : null}
      </div>
    </section>
  );
}

/** A group's id, status, branch and actions (cancel, retry the merge). */
function GroupBar({ group, state }: { group: Group; state: State }) {
  const store = useStore();
  const p = groupProgress(state, group.id);
  const cancellable = group.status === "active" || group.status === "merge_blocked";
  return (
    <section aria-label={`group ${group.id}`} className="group-bar-wrap">
      <div className="group-bar">
        <span className={`badge group-status-${group.status}`} data-testid={`group-status-${group.id}`}>
          {groupLabel(state, group)}
        </span>
        <span>{group.id}</span>
        <span className="dim" title="グループブランチ ← base">
          {group.group_branch} ← {group.base_branch}
        </span>
        <span>
          {p.done}/{p.total} 完了{p.handling > 0 ? ` · ${p.handling} 件 対処中` : ""}
        </span>
        <span className="spacer" />
        {cancellable ? (
          <button type="button" className="btn btn-small" onClick={() => void store.cancelGroup(group.id)}>
            グループを中止
          </button>
        ) : null}
      </div>
      {group.status === "merge_blocked" ? (
        <div className="group-alert" role="alert">
          <span>base ブランチへのマージが保留されています: {group.detail ?? "理由不明"}</span>
          <button type="button" className="btn btn-small" onClick={() => void store.retryGroupMerge(group.id)}>
            マージを再試行
          </button>
        </div>
      ) : null}
      {group.detail && group.status !== "merge_blocked" ? <div className="task-meta">{group.detail}</div> : null}
    </section>
  );
}
