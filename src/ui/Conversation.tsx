import { useMemo, useRef } from "react";

import type { ChatInfo, GitOverview, State } from "../api/generated";
import { orchestratorKey, scopeState } from "../store/chats";
import { orchestratorSession, type ProjectView, selectedChatInfo } from "../store/project";
import { useAppState, useStore } from "../store/useStore";
import { Composer } from "./Composer";
import { EmptyState } from "./EmptyState";
import { Icon, IconButton } from "./Icon";
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
const NO_CHAT_HINT = "サイドバーの「新しいチャット」から始めます";

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
        <Icon name="message" size={14} className="panel-icon" />
        <span className="panel-title" data-testid="chat-title" title={chat?.title ?? undefined}>
          {chat ? (chat.title ?? NEW_CHAT_TITLE) : "orchestrator"}
        </span>
        {chat && placeText ? (
          <span className="chip branch-chip" title={`このチャットの作業ツリー: ${chat.worktree}`} data-testid="chat-branch">
            <Icon name={place.kind === "missing" ? "alert" : "git-branch"} size={11} />
            {placeText}
          </span>
        ) : null}
        <span className="spacer" />
        <SessionBadge status={session?.status} running={session?.turn_running ?? false} />
      </header>
      <div className="scroll" ref={scroller}>
        {!chat ? (
          <EmptyState icon="message-plus" testId="no-chat" text={NO_CHAT_HINT} />
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
          <EmptyState icon="message" text="会話はまだありません" />
        )}
      </div>
      {scoped ? <WaitBanner state={scoped} /> : null}
      <Composer
        key={chat?.id ?? "none"}
        disabled={disabledReason !== null}
        disabledReason={chat ? disabledReason : null}
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
  const loading = view.loadingOlder;
  return (
    <IconButton
      icon={loading ? "loader" : "chevrons-up"}
      spin={loading}
      className="load-older"
      label={loading ? "読み込み中…" : `さらに前の履歴を読み込む (#${view.historyStart - 1} まで)`}
      disabled={loading || view.phase !== "ready"}
      onClick={() => void store.loadOlderHistory()}
    />
  );
}

/**
 * The design's group-wait banner (§3.4): while a group runs, results reach the
 * orchestrator only when every task has settled. The sentence is its tooltip;
 * on screen: the group, how far it is, and how many tasks need handling.
 */
function WaitBanner({ state }: { state: State }) {
  const group = focusGroup(state);
  if (!group || !isOpenGroup(group)) return null;
  const p = groupProgress(state, group.id);
  const finishing = group.status === "finishing";
  const awaiting = awaitingFinish(state, group);
  const text = finishing
    ? `グループ「${group.title}」を base ブランチへマージ中`
    : awaiting
      ? `グループ「${group.title}」の全タスクが完了 — オーケストレータがグループを完了 (マージ) するのを待っています`
      : `グループ「${group.title}」の全タスク完了まで待機中 — ${p.done}/${p.total} 完了${p.handling > 0 ? ` · ${p.handling} 件 対処中` : ""}`;
  const share = p.total > 0 ? Math.round((p.done / p.total) * 100) : 0;
  return (
    <div className="wait-banner" role="status" data-testid="wait-banner" title={text} aria-label={text}>
      <Icon name={finishing ? "git-merge" : awaiting ? "hourglass" : "layers"} size={14} className={finishing ? "wait-busy" : ""} />
      <span className="wait-title">{group.title}</span>
      <span className="wait-progress" aria-hidden="true">
        <span style={{ width: `${share}%` }} />
      </span>
      <span className="wait-count">
        {p.done}/{p.total}
      </span>
      {p.handling > 0 ? (
        <span className="wait-handling">
          <Icon name="alert" size={12} />
          {p.handling}
        </span>
      ) : null}
    </div>
  );
}

/**
 * The orchestrator's state, by shape as well as colour: a hollow ring not
 * started, a filled green dot idle, a blue dot with a halo working, and the
 * alert icon when the session needs attention. The words are the tooltip.
 */
function SessionBadge({ status, running }: { status: string | undefined; running: boolean }) {
  const label = !status ? "not started" : status === "live" ? (running ? "working" : "idle") : status;
  const title = `オーケストレータ: ${label}`;
  const words = <span className="sr-only">{label}</span>;
  if (status && status !== "live") {
    return (
      <span className="status-icon status-icon-bad" title={title} data-testid="orchestrator-status">
        <Icon name="alert" size={14} />
        {words}
      </span>
    );
  }
  const tone = !status ? "" : running ? " status-dot-working" : " status-dot-idle";
  return (
    <span className={`status-dot${tone}`} title={title} data-testid="orchestrator-status">
      {words}
    </span>
  );
}
