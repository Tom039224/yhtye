import { useMemo, useRef } from "react";

import type { ChatInfo, GitOverview, State } from "../api/generated";
import { orchestratorKey, scopeState } from "../store/chats";
import { orchestratorSession, type ProjectView, selectedChatInfo } from "../store/project";
import { useAppState, useStore } from "../store/useStore";
import { Composer } from "./Composer";
import { withMentions, type Mention } from "./mentions";
import { type Place, placeLabel, placeOf } from "./branchTree";
import { awaitingFinish, focusGroup, groupProgress, isOpenGroup } from "./taskInfo";
import { TranscriptView } from "./TranscriptView";
import { useAutoScroll } from "./useAutoScroll";

interface Props {
  view: ProjectView;
  mentions: Mention[];
  onRemoveMention: (task: string) => void;
  onClearMentions: () => void;
}

export const NEW_CHAT_TITLE = "新しいチャット";
const NO_CHAT_HINT = "BRANCHES の「+ 新しいチャット」から始めます";

/** Where the chat's worktree is now (its branch, detached, gone; unknown until git is read). */
function chatPlace(overview: GitOverview | null, chat: ChatInfo | null): Place {
  return chat ? placeOf(chat, overview) : { kind: "unknown" };
}

export function Conversation({ view, mentions, onRemoveMention, onClearMentions }: Props) {
  const store = useStore();
  const connection = useAppState((s) => s.connection);
  const sending = useAppState((s) => s.busy.sending);
  const chat = selectedChatInfo(view) ?? null;
  const key = chat ? orchestratorKey(chat.id) : null;
  const items = key ? view.transcripts[key] : undefined;
  const streaming = key ? view.streaming[key] : undefined;
  const scoped = useMemo(() => (view.state ? scopeState(view.state, chat?.id ?? null) : null), [view.state, chat?.id]);
  const queued = useMemo(() => new Set((scoped?.inbox ?? []).map((e) => e.id)), [scoped]);
  const session = orchestratorSession(view, chat?.id ?? null);
  const overview = useAppState((s) => (s.git?.project === view.info.id ? s.git.overview : null));
  const place = chatPlace(overview, chat);
  const placeText = placeLabel(place);
  const scroller = useRef<HTMLDivElement>(null);
  useAutoScroll(scroller, [items, streaming], items?.[0]?.seq, chat?.id);

  const disabledReason =
    !chat
      ? NO_CHAT_HINT
      : connection.state !== "open"
      ? "コアに接続していません"
      : view.phase === "loading"
        ? "読み込み中…"
        : view.phase === "error"
          ? "プロジェクトを読み込めませんでした"
          : place.kind === "missing"
            ? `作業ツリー「${chat.worktree}」が見つからないため送信できません (履歴は読めます)`
            : null;

  const send = async (text: string) => {
    const ok = await store.sendMessage(withMentions(text, mentions));
    if (ok) onClearMentions();
    return ok;
  };

  return (
    <section className="conversation" aria-label="orchestrator">
      <header className="panel-header">
        <span className="panel-title" data-testid="chat-title" title={chat?.title ?? undefined}>
          {chat ? (chat.title ?? NEW_CHAT_TITLE) : "orchestrator"}
        </span>
        {chat && placeText ? (
          <span className="chip branch-chip" title={`このチャットの作業ツリー: ${chat.worktree}`} data-testid="chat-branch">
            {place.kind === "missing" ? placeText : `⎇ ${placeText}`}
          </span>
        ) : null}
        <span className="spacer" />
        <SessionBadge status={session?.status} running={session?.turn_running ?? false} />
      </header>
      <div className="scroll" ref={scroller}>
        {!chat ? (
          <p className="empty" data-testid="no-chat">{NO_CHAT_HINT}</p>
        ) : items?.length || streaming || view.historyStart > 1 ? (
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
      {scoped ? <WaitBanner state={scoped} /> : null}
      <Composer
        key={chat?.id ?? "none"}
        disabled={disabledReason !== null}
        disabledReason={disabledReason}
        sending={sending}
        turnRunning={session?.turn_running ?? false}
        starting={queued.size > 0 && session?.status !== "live"}
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
      : awaitingFinish(state, group)
        ? `グループ「${group.title}」の全タスクが完了 — オーケストレータがグループを完了 (マージ) するのを待っています`
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
