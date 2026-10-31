import { useMemo, useRef, useSyncExternalStore } from "react";

import type { ChatInfo, GitOverview, State } from "../api/generated";
import type { AppState } from "../store/app";
import { orchestratorKey, scopeState } from "../store/chats";
import type { DraftAttachment } from "../store/drafts";
import { orchestratorSession, type ProjectView, selectedChatInfo } from "../store/project";
import { useAppState, useStore } from "../store/useStore";
import { Composer } from "./Composer";
import { type CompactState, ContextBar } from "./ContextBar";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { withMentions, type Mention } from "./mentions";
import { OlderHistory } from "./OlderHistory";
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
const NONE: string[] = [];
const NO_ATTACHMENTS: readonly DraftAttachment[] = [];
const NO_CHAT_HINT = "サイドバーの「新しいチャット」から始めます";

/** Where the chat's worktree is now (its branch, detached, gone; unknown until git is read). */
function chatPlace(overview: GitOverview | null, chat: ChatInfo | null, awaiting: string[]): Place {
  return chat ? placeOf(chat, overview, awaiting.includes(chat.id)) : { kind: "unknown" };
}

/**
 * Where the chat's worktree is now, as what the conversation shows of it (the
 * kind and the label): a git read that changes neither does not render it again.
 */
function useChatPlace(project: string, chat: ChatInfo | null): { kind: Place["kind"]; text: string | null } {
  const where = (s: AppState): Place => {
    const git = s.git?.project === project ? s.git : null;
    return chatPlace(git?.overview ?? null, chat, git?.awaiting ?? NONE);
  };
  const kind = useAppState((s) => where(s).kind);
  const text = useAppState((s) => placeLabel(where(s)));
  return { kind, text };
}

export function Conversation({ view, mentions, onRemoveMention, onClearMentions }: Props) {
  const store = useStore();
  const connection = useAppState((s) => s.connection);
  const sending = useAppState((s) => s.busy.sending);
  const chat = selectedChatInfo(view) ?? null;
  const key = chat ? orchestratorKey(chat.id) : null;
  const items = key ? view.transcripts[key] : undefined;
  // Only this chat's own text: a chunk of another session does not render this again.
  const streaming = useAppState((s) => (key ? s.live.streaming[key] : undefined));
  const scoped = useMemo(() => (view.state ? scopeState(view.state, chat?.id ?? null) : null), [view.state, chat?.id]);
  const queued = useMemo(() => new Set((scoped?.inbox ?? []).map((e) => e.id)), [scoped]);
  const session = orchestratorSession(view, chat?.id ?? null);
  const resuming = chat ? Boolean(view.resuming[chat.id]) : false;
  const project = view.info.id;
  const place = useChatPlace(project, chat);
  const placeText = place.text;
  const attachments = useSyncExternalStore(store.drafts.subscribe, () =>
    chat ? store.drafts.attachments(project, chat.id) : NO_ATTACHMENTS,
  );
  const chatId = chat?.id ?? null;
  const loadAttachment = useMemo(
    () => (chatId ? (id: string) => store.attachments.preview(project, chatId, id) : undefined),
    [store, project, chatId],
  );
  const scroller = useRef<HTMLDivElement>(null);
  // `scoped`: a group card grows with its tasks and a queued message loses its mark, with no change of the items.
  useAutoScroll(scroller, [items, streaming, scoped], items?.[0]?.seq, chat?.id);

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

  const compacting = useAppState((s) => s.busy.compacting);
  const usage = useAppState((s) => (key ? (s.live.contextUsage[key] ?? null) : null));
  const compact = compactState({
    disabledReason,
    status: session?.status,
    turnRunning: session?.turn_running ?? false,
    compacting: compacting || (key !== null && view.compacting[key] === true),
  });

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
        <SessionBadge status={session?.status} running={session?.turn_running ?? false} starting={resuming} />
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
            loadAttachment={loadAttachment}
            before={<OlderHistory view={view} scroller={scroller} />}
          />
        ) : (
          <EmptyState icon="message" text="会話はまだありません" />
        )}
      </div>
      {scoped ? <WaitBanner state={scoped} /> : null}
      <Composer
        key={chat ? `${store.scopedKey(view.info.id)}/${chat.id}` : "none"}
        initialText={chat ? store.drafts.get(view.info.id, chat.id) : ""}
        onTextChange={(text) => {
          if (chat) store.drafts.set(view.info.id, chat.id, text);
        }}
        disabled={disabledReason !== null}
        disabledReason={chat ? disabledReason : null}
        sending={sending}
        turnRunning={session?.turn_running ?? false}
        starting={resuming || (queued.size > 0 && session?.status !== "live")}
        mentions={mentions}
        onRemoveMention={onRemoveMention}
        attachments={attachments}
        onAddFiles={(files) => {
          if (chat) void store.attachments.attach(project, chat.id, files);
        }}
        onRemoveAttachment={(key) => {
          if (chat) void store.attachments.remove(project, chat.id, key);
        }}
        onSend={send}
        onCancel={() => void store.cancelTurn()}
        footer={chat ? <ContextBar usage={usage} compact={compact} onCompact={() => void store.compactChat()} /> : null}
      />
    </section>
  );
}

/**
 * Whether the compact button can be pressed: only for a running, idle
 * orchestrator (the core refuses otherwise); the reason is its tooltip.
 */
export function compactState(o: {
  disabledReason: string | null;
  status: string | undefined;
  turnRunning: boolean;
  compacting: boolean;
}): CompactState {
  if (o.compacting) return { kind: "compacting" };
  if (o.disabledReason) return { kind: "disabled", reason: o.disabledReason };
  if (o.status !== "live") return { kind: "disabled", reason: "オーケストレータが起動していないため圧縮できません" };
  if (o.turnRunning) return { kind: "disabled", reason: "作業中は圧縮できません (ターン終了後に押せます)" };
  return { kind: "ready" };
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
 * running (never started, or suspended until the chat is opened or gets
 * something), a spinner while it starts, a filled green dot idle, a blue dot
 * with a halo working, and the alert icon when the session needs attention.
 * The words are the tooltip.
 */
function SessionBadge({ status, running, starting }: { status: string | undefined; running: boolean; starting: boolean }) {
  const label = starting
    ? "starting"
    : !status
      ? "not started"
      : status === "live"
        ? running
          ? "working"
          : "idle"
        : status;
  const title = `オーケストレータ: ${label}`;
  const words = <span className="sr-only">{label}</span>;
  if (starting) {
    return (
      <span className="status-icon" title={title} data-testid="orchestrator-status">
        <Icon name="loader" size={14} spin />
        {words}
      </span>
    );
  }
  if (status && status !== "live" && !PAUSED.has(status)) {
    return (
      <span className="status-icon status-icon-bad" title={title} data-testid="orchestrator-status">
        <Icon name="alert" size={14} />
        {words}
      </span>
    );
  }
  const tone = !status || PAUSED.has(status) ? "" : running ? " status-dot-working" : " status-dot-idle";
  return (
    <span className={`status-dot${tone}`} title={title} data-testid="orchestrator-status">
      {words}
    </span>
  );
}

/**
 * Sessions that are not running because Yhtye was restarted: the orchestrator is
 * brought back when the chat is opened or has something to receive, so this is
 * not a problem to flag.
 */
const PAUSED: ReadonlySet<string> = new Set(["suspended", "interrupted"]);
