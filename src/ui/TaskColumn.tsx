import { useMemo } from "react";

import type { Group, State } from "../api/generated";
import { scopeState } from "../store/chats";
import type { ProjectView } from "../store/project";
import { useStore } from "../store/useStore";
import { EmptyState } from "./EmptyState";
import { Icon, IconButton } from "./Icon";
import type { Mention } from "./mentions";
import { groupIcon, StatusChip } from "./statusIcons";
import { TaskCard } from "./TaskCard";
import { awaitingFinish, focusGroup, groupLabel, groupProgress, liveSubagents } from "./taskInfo";
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
        <Icon name="list" size={14} className="panel-icon" />
        <span className="sr-only">tasks</span>
        {group ? <span className="chip" title={group.id}>{group.title}</span> : null}
        <span className="spacer" />
        <span className="panel-meta" title="動いているサブエージェントのセッション数">
          <Icon name="bot" size={14} />
          <span className="sr-only">subagents </span>
          {state ? liveSubagents(view, state) : 0}
        </span>
      </header>
      <div className="scroll">
        {view.phase === "loading" && !state ? <EmptyState icon="loader" text="読み込み中…" /> : null}
        {view.phase === "error" ? <EmptyState icon="alert" tone="error" text={view.loadError ?? "読み込めませんでした"} /> : null}
        {state && !group ? (
          <EmptyState
            icon="list"
            text={selectedChat ? "グループはまだありません" : "チャットを選ぶとタスクが出ます"}
          />
        ) : null}
        {state && group ? (
          <div className="task-list">
            <GroupBar group={group} state={state} />
            {cards(group, state)}
            {state.tasks.every((t) => t.group !== group.id) ? <EmptyState icon="list" text="タスクはまだありません" /> : null}
            {older.length > 0 ? (
              <details className="past-groups fold">
                <summary title="過去のグループ" aria-label={`過去のグループ (${older.length})`}>
                  <Icon name="chevron-down" size={12} className="chevron" />
                  <Icon name="history" size={13} />
                  {older.length}
                </summary>
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
        <StatusChip
          className={`group-status-${group.status}`}
          icon={groupIcon(group.status, awaitingFinish(state, group))}
          label={groupLabel(state, group)}
          testId={`group-status-${group.id}`}
        />
        <span className="group-id">{group.id}</span>
        <span className="group-branches" title="グループブランチ ← マージ先 (base)">
          <Icon name="git-branch" size={12} />
          {group.group_branch}
          <Icon name="arrow-left" size={11} />
          {group.base_branch}
        </span>
        <span className="spacer" />
        <span className="group-count" title={`${p.done}/${p.total} 完了`}>
          <Icon name="check" size={12} />
          {p.done}/{p.total}
          <span className="sr-only"> 完了</span>
        </span>
        {p.handling > 0 ? (
          <span className="group-count group-count-handling" title={`${p.handling} 件 対処中`}>
            <Icon name="alert" size={12} />
            {p.handling}
            <span className="sr-only"> 件 対処中</span>
          </span>
        ) : null}
        {cancellable ? (
          <IconButton icon="stop" label="グループを中止" onClick={() => void store.cancelGroup(group.id)} />
        ) : null}
      </div>
      {group.status === "merge_blocked" ? (
        <div className="group-alert" role="alert">
          <Icon name="alert" size={14} />
          <span>base ブランチへのマージが保留されています: {group.detail ?? "理由不明"}</span>
          <IconButton icon="refresh" label="マージを再試行" onClick={() => void store.retryGroupMerge(group.id)} />
        </div>
      ) : null}
      {group.detail && group.status !== "merge_blocked" ? <div className="task-meta">{group.detail}</div> : null}
    </section>
  );
}
