import { memo, type ReactNode } from "react";

import type { State } from "../api/generated";
import type { TranscriptItem } from "../store/transcript";
import { Icon, type IconName } from "./Icon";
import { formatTime, taskTone } from "./labels";
import { Markdown, StreamingMarkdown } from "./Markdown";
import { type LoadAttachment, SentAttachments } from "./SentAttachments";
import { ToolStatus, toneIcon } from "./statusIcons";

interface Props {
  items: TranscriptItem[];
  streaming?: { message: string; thought: string };
  /** Names the agent's messages ("orchestrator", "implementer", ...) in their tooltip. */
  agentLabel: string;
  /** Undelivered inbox entry ids (user messages still queued). */
  queued?: ReadonlySet<number>;
  /** For the group cards of the conversation (their tasks are read live). */
  state?: State | null;
  /** Shown first (e.g. the button that loads older history). */
  before?: ReactNode;
  /** Reads the images the user attached (the conversation's chat). */
  loadAttachment?: LoadAttachment;
}

export function TranscriptView({ items, streaming, agentLabel, queued, state, before, loadAttachment }: Props) {
  return (
    <div className="transcript">
      {before}
      <Items items={items} agentLabel={agentLabel} queued={queued} state={state} loadAttachment={loadAttachment} />
      <Streamed streaming={streaming} agentLabel={agentLabel} />
    </div>
  );
}

/**
 * The stored items. Rendering a long conversation is the costly part of the
 * screen, so it is skipped while nothing it shows has changed (a chunk of the
 * text being streamed below it, a git refresh): the list is memoized, and so is
 * each item, which is given only the props its kind reads (`waiting` for a user
 * message, `state` for a group card) so that a change of the state or of the
 * inbox does not render the others again.
 */
const Items = memo(function Items({
  items,
  agentLabel,
  queued,
  state,
  loadAttachment,
}: Pick<Props, "items" | "agentLabel" | "queued" | "state" | "loadAttachment">) {
  return items.map((item) => (
    // Items are appended, or prepended when older history loads; a durable
    // event yields at most one item per session, so seq identifies it.
    <Item
      key={`${item.seq}-${item.kind}`}
      item={item}
      agentLabel={agentLabel}
      waiting={item.kind === "user" && (queued?.has(item.inboxId) ?? false)}
      state={item.kind === "group" ? (state ?? null) : null}
      loadAttachment={item.kind === "user" ? loadAttachment : undefined}
    />
  ));
});

/** The text of the agent's turn that is not stored yet. */
function Streamed({ streaming, agentLabel }: { streaming: Props["streaming"]; agentLabel: string }) {
  return (
    <>
      {streaming?.thought ? <Thought text={streaming.thought} streaming /> : null}
      {streaming?.message ? (
        <div className="msg msg-agent" data-testid="streaming">
          <Mark icon="bot" label={agentLabel} streaming />
          <div className="msg-body msg-md">
            <StreamingMarkdown text={streaming.message} />
          </div>
        </div>
      ) : null}
    </>
  );
}

/** Who speaks: an icon in the margin, the name in its tooltip and for assistive tech. */
function Mark({ icon, label, time, streaming = false }: { icon: IconName; label: string; time?: number; streaming?: boolean }) {
  return (
    <span className={`msg-mark ${icon === "bot" ? "msg-mark-agent" : ""} ${streaming ? "msg-mark-streaming" : ""}`} title={time === undefined ? label : `${label} · ${formatTime(time)}`}>
      <Icon name={icon} size={14} />
      <span className="sr-only">{label}</span>
    </span>
  );
}

function Time({ ts }: { ts: number }) {
  return <time className="msg-time">{formatTime(ts)}</time>;
}

function Thought({ text, streaming }: { text: string; streaming?: boolean }) {
  const label = streaming ? "thinking…" : "thought";
  return (
    <details className="thought fold">
      <summary title={streaming ? "考え中" : "思考"} className={streaming ? "thinking" : ""}>
        <Icon name="chevron-down" size={12} className="chevron" />
        <Icon name="thought" size={13} />
        <span className="sr-only">{label}</span>
      </summary>
      <div className="thought-body">{text}</div>
    </details>
  );
}

function GroupCard({ id, title, state }: { id: string; title: string; state: State | null }) {
  const tasks = state?.tasks.filter((t) => t.group === id) ?? [];
  return (
    <div className="group-card" aria-label={`group card ${id}`}>
      <div className="group-card-header" title="グループ">
        <Icon name="layers" size={13} />
        <span className="group-card-title">{title}</span>
        <span className="dim" title={id}>
          {id}
        </span>
      </div>
      <div className="group-card-body">
        {tasks.length === 0 ? <div className="dim">タスクはまだありません</div> : null}
        {tasks.map((t) => {
          const tone = taskTone(t);
          return (
            <div key={t.id} className="group-card-row">
              <Icon name={toneIcon(tone, t.kind)} size={12} className={`tone-icon tone-icon-${tone}`} />
              <span className="n" title={t.id}>
                {t.id}
              </span>
              <span className="group-card-row-title">{t.title}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

const Item = memo(function Item({
  item,
  agentLabel,
  waiting,
  state,
  loadAttachment,
}: {
  item: TranscriptItem;
  agentLabel: string;
  /** A user message that is still queued (not delivered yet). */
  waiting: boolean;
  /** The group cards' state (`null` for any other item). */
  state: State | null;
  /** The user messages' attachments (`undefined` for any other item). */
  loadAttachment?: LoadAttachment;
}) {
  switch (item.kind) {
    case "group":
      return <GroupCard id={item.groupId} title={item.title} state={state} />;
    case "user": {
      const mark = waiting ? (
        <span className="queued" title="待機中 (オーケストレータの手が空いたら届く)">
          <Icon name="clock" size={12} />
          <span className="sr-only">待機中</span>
        </span>
      ) : null;
      const files = item.attachments.length > 0;
      // The text's bubble, then the attachments below it (a message may be attachments only).
      const bubble =
        item.text !== "" || !files ? (
          <div className="msg-body">
            {item.text}
            {mark}
          </div>
        ) : null;
      return (
        <div className="msg msg-user">
          <Mark icon="user" label="あなた" time={item.ts} />
          {files ? (
            <div className="msg-user-stack">
              {bubble}
              <SentAttachments attachments={item.attachments} load={loadAttachment} />
              {bubble ? null : mark}
            </div>
          ) : (
            bubble
          )}
          <Time ts={item.ts} />
        </div>
      );
    }
    case "delivered":
      return null;
    case "notice":
      return (
        <div className="notice" title={`yhtye → ${agentLabel}`}>
          <Icon name="bell" size={12} />
          <span className="mono">{item.inboxKind}</span>
          {item.attrs.length > 0 ? <span className="mono dim"> {item.attrs.map(([k, v]) => `${k}=${v}`).join(" ")}</span> : null}
          {item.body ? <div className="notice-body">{item.body}</div> : null}
        </div>
      );
    case "prompt":
      return (
        <details className="prompt fold">
          <summary className="mono" title="エージェントへのプロンプト">
            <Icon name="chevron-down" size={12} className="chevron" />
            <Icon name="file-text" size={13} />
            <span className="sr-only">prompt</span>
            <time>{formatTime(item.ts)}</time>
          </summary>
          <div className="prompt-body">{item.text}</div>
        </details>
      );
    case "orchestrator_message":
      return (
        <div className="orch-msg" aria-label="orchestrator message">
          <div className="orch-msg-head">
            <Icon name="message" size={12} />
            <span className="orch-msg-label">オーケストレータから</span>
            {item.interrupted ? (
              <span className="orch-msg-interrupted" title="エージェントのターンを止めて、このメッセージを送り直した (それまでの作業は残っている)">
                割り込みで再送
              </span>
            ) : null}
            <Time ts={item.ts} />
          </div>
          <div className="orch-msg-body msg-md">
            <Markdown text={item.text} />
          </div>
        </div>
      );
    case "text":
      return item.textKind === "thought" ? (
        <Thought text={item.text} />
      ) : (
        <div className="msg msg-agent">
          <Mark icon="bot" label={agentLabel} time={item.ts} />
          <div className="msg-body msg-md">
            <Markdown text={item.text} />
          </div>
          <Time ts={item.ts} />
        </div>
      );
    case "tool":
      return (
        <div className="tool mono">
          <Icon name="wrench" size={12} className="tool-kind" />
          <span className="tool-title">{item.title}</span>
          {item.calls.length > 0 ? <span className="tool-calls"> → {item.calls.join(", ")}</span> : null}
          <ToolStatus status={item.status} />
        </div>
      );
    case "yhtye_tool":
      return (
        <details className={`tool fold mono ${item.ok ? "" : "error-text"}`}>
          <summary title="Yhtye のツール呼び出し">
            <Icon name="chevron-down" size={12} className="chevron" />
            <Icon name="bolt" size={12} className="tool-kind" />
            <span className="tool-title">{item.tool}</span>
            {item.by === "user" ? <Icon name="user" size={11} className="tool-by" /> : null}
            <span className={`tool-status tool-status-${item.ok ? "completed" : "failed"}`}>
              <Icon name={item.ok ? "check" : "alert"} size={12} />
              <span className="sr-only">{item.ok ? "ok" : "error"}</span>
            </span>
          </summary>
          <div className="tool-detail">{item.detail}</div>
        </details>
      );
    case "turn":
      return (
        <div className={`turn mono ${item.error ? "error-text" : "dim"}`}>
          <Icon name={item.error ? "alert" : "stop"} size={12} />
          turn ended: {item.outcome}
        </div>
      );
    case "compact":
      return (
        <div className="lifecycle mono dim" data-testid="compact-item">
          <Icon name="minimize" size={12} />
          コンテキストを圧縮しました (/compact)
        </div>
      );
    case "lifecycle":
      return (
        <div className={`lifecycle mono ${item.error ? "error-text" : "dim"}`}>
          <Icon name={item.error ? "alert" : "info"} size={12} />
          {item.text}
        </div>
      );
  }
});
