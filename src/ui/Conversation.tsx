import { useMemo, useRef } from "react";

import type { State } from "../api/generated";
import { orchestratorSession, type ProjectView } from "../store/project";
import { ORCHESTRATOR } from "../store/transcript";
import { useAppState, useStore } from "../store/useStore";
import { Composer } from "./Composer";
import { withMentions, type Mention } from "./mentions";
import { focusGroup, groupProgress, isOpenGroup } from "./taskInfo";
import { TranscriptView } from "./TranscriptView";
import { useAutoScroll } from "./useAutoScroll";

interface Props {
  view: ProjectView;
  mentions: Mention[];
  onRemoveMention: (task: string) => void;
  onClearMentions: () => void;
}

export function Conversation({ view, mentions, onRemoveMention, onClearMentions }: Props) {
  const store = useStore();
  const connection = useAppState((s) => s.connection);
  const sending = useAppState((s) => s.busy.sending);
  const items = view.transcripts[ORCHESTRATOR];
  const streaming = view.streaming[ORCHESTRATOR];
  const queued = useMemo(() => new Set((view.state?.inbox ?? []).map((e) => e.id)), [view.state?.inbox]);
  const session = orchestratorSession(view);
  const scroller = useRef<HTMLDivElement>(null);
  useAutoScroll(scroller, [items, streaming], items?.[0]?.seq);

  const disabledReason =
    connection.state !== "open"
      ? "コアに接続していません"
      : view.phase === "loading"
        ? "読み込み中…"
        : view.phase === "error"
          ? "プロジェクトを読み込めませんでした"
          : null;

  const send = async (text: string) => {
    const ok = await store.sendMessage(withMentions(text, mentions));
    if (ok) onClearMentions();
    return ok;
  };

  return (
    <section className="conversation" aria-label="orchestrator">
      <header className="panel-header">
        <span className="panel-title">orchestrator</span>
        <SessionBadge status={session?.status} running={session?.turn_running ?? false} />
      </header>
      <div className="scroll" ref={scroller}>
        {items?.length || streaming || view.historyStart > 1 ? (
          <TranscriptView
            items={items ?? []}
            streaming={streaming}
            agentLabel="orchestrator"
            queued={queued}
            state={view.state}
            before={<OlderHistory view={view} />}
          />
        ) : (
          <p className="empty">まだ会話はありません。下の欄からオーケストレータに依頼してください。</p>
        )}
      </div>
      {view.state ? <WaitBanner state={view.state} /> : null}
      <Composer
        disabled={disabledReason !== null}
        disabledReason={disabledReason}
        sending={sending}
        turnRunning={session?.turn_running ?? false}
        mentions={mentions}
        onRemoveMention={onRemoveMention}
        onSend={send}
        onCancel={() => void store.cancelTurn()}
      />
    </section>
  );
}

/** "Load older history" while the start of the log is not loaded (lazy history). */
export function OlderHistory({ view }: { view: ProjectView }) {
  const store = useStore();
  if (view.historyStart <= 1) return null;
  return (
    <button
      type="button"
      className="btn btn-small load-older"
      disabled={view.loadingOlder || view.phase !== "ready"}
      onClick={() => void store.loadOlderHistory()}
    >
      {view.loadingOlder ? "読み込み中…" : `さらに前の履歴を読み込む (#${view.historyStart - 1} まで)`}
    </button>
  );
}

/**
 * The design's group-wait banner (§3.4): while a group runs, results reach the
 * orchestrator only when every task has settled.
 */
function WaitBanner({ state }: { state: State }) {
  const group = focusGroup(state);
  if (!group || !isOpenGroup(group)) return null;
  const p = groupProgress(state, group.id);
  const text =
    group.status === "finishing"
      ? `グループ「${group.title}」を base ブランチへマージ中`
      : `グループ「${group.title}」の全タスク完了まで待機中 — ${p.done}/${p.total} 完了${p.handling > 0 ? ` · ${p.handling} 件 対処中` : ""}`;
  return (
    <div className="wait-banner" role="status" data-testid="wait-banner">
      <span className="dot dot-accent" />
      {text}
    </div>
  );
}

function SessionBadge({ status, running }: { status: string | undefined; running: boolean }) {
  if (!status) return <span className="badge tone-waiting" data-testid="orchestrator-status">not started</span>;
  const label = status === "live" ? (running ? "working" : "idle") : status;
  const tone = status === "live" ? (running ? "tone-implementing" : "tone-done") : "tone-handling";
  return (
    <span className={`badge ${tone}`} data-testid="orchestrator-status">
      {running ? <span className="badge-dot" /> : null}
      {label}
    </span>
  );
}
