import { type KeyboardEvent, useState } from "react";

import { Icon, IconButton } from "./Icon";
import type { Mention } from "./mentions";

interface Props {
  disabled: boolean;
  /** Why sending is not possible (shown instead of the hint). */
  disabledReason: string | null;
  sending: boolean;
  turnRunning: boolean;
  /** Sent, but the orchestrator process is still starting (lazy start). */
  starting?: boolean;
  mentions: Mention[];
  onRemoveMention: (task: string) => void;
  /** Sends `text` (mentions are added by the caller); resolves to whether it was accepted. */
  onSend: (text: string) => Promise<boolean>;
  onCancel: () => void;
}

/**
 * The design's composer (§3.4): text, then a row of mention chips and the send
 * button. Enter sends, Shift+Enter inserts a newline (IME
 * composition is left alone). During an orchestrator turn messages are still
 * accepted: they wait in its inbox and are delivered when the turn ends.
 */
export function Composer({
  disabled,
  disabledReason,
  sending,
  turnRunning,
  starting = false,
  mentions,
  onRemoveMention,
  onSend,
  onCancel,
}: Props) {
  const [text, setText] = useState("");
  const canSend = !disabled && !sending && text.trim().length > 0;

  const send = async () => {
    if (!canSend) return;
    const sent = await onSend(text);
    if (sent) setText("");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void send();
  };

  const hint = disabledReason ?? (starting ? "起動中…" : turnRunning ? "作業中 · 送信分はターン終了後に届きます" : null);

  return (
    <div className="composer-wrap">
      <div className="composer">
        <textarea
          aria-label="オーケストレータへのメッセージ"
          placeholder="オーケストレータに指示する…"
          value={text}
          disabled={disabled}
          rows={Math.min(8, Math.max(2, text.split("\n").length))}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="composer-row">
          {mentions.map((m) => (
            <button
              type="button"
              key={m.task}
              className="mention"
              title={`${m.task} ${m.title} (クリックで外す)`}
              aria-label={`${m.task} の引用を外す`}
              onClick={() => onRemoveMention(m.task)}
            >
              <Icon name="at" size={12} className="at" />
              <span className="label">
                {m.task} {m.title}
              </span>
              <Icon name="x" size={10} className="x" />
            </button>
          ))}
          <span className="spacer" />
          {turnRunning ? (
            <IconButton icon="stop" tone="danger" label="ターンを中止" onClick={onCancel} disabled={disabled} />
          ) : null}
          <IconButton
            icon="send"
            tone="primary"
            label={sending ? "送信中…" : "送信"}
            title="送信 (Enter) · 改行は Shift+Enter"
            onClick={() => void send()}
            disabled={!canSend}
          />
          {hint ? (
            <span className={`composer-hint ${disabledReason ? "error-text" : ""}`} role="status">
              <Icon name={disabledReason ? "alert" : starting ? "loader" : "clock"} size={12} spin={!disabledReason && starting} />
              {hint}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  );
}
