import { type KeyboardEvent, useLayoutEffect, useRef, useState } from "react";

import { Icon, IconButton } from "./Icon";
import type { Mention } from "./mentions";

/** The textarea grows with its text from one row up to this many rows, then scrolls. */
const MIN_ROWS = 1;
const MAX_ROWS = 11;
/** The textarea's `line-height` (App.css), to turn `MAX_ROWS` into a `max-height`. */
const LINE_HEIGHT_EM = 1.6;

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
 * The design's composer (§3.4): the text with the send button at its lower
 * right (the text uses the full height; the button takes width only), then a
 * row of mention chips. Enter sends, Shift+Enter inserts a newline (IME
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
  const textarea = useRef<HTMLTextAreaElement>(null);
  const canSend = !disabled && !sending && text.trim().length > 0;

  const send = async () => {
    if (!canSend) return;
    const sent = await onSend(text);
    if (sent) setText("");
  };

  // Wrapped lines do not add rows, so fit the height to the content (capped by CSS `max-height`).
  useLayoutEffect(() => {
    const el = textarea.current;
    if (!el) return;
    el.style.height = "auto";
    if (el.scrollHeight > 0) el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void send();
  };

  const hint = disabledReason ?? (starting ? "起動中…" : turnRunning ? "作業中 · 送信分はターン終了後に届きます" : null);

  return (
    <div className="composer-wrap">
      <div className="composer">
        <div className="composer-body">
          <textarea
            ref={textarea}
            aria-label="オーケストレータへのメッセージ"
            placeholder="オーケストレータに指示する…"
            value={text}
            disabled={disabled}
            rows={Math.min(MAX_ROWS, Math.max(MIN_ROWS, text.split("\n").length))}
            style={{ maxHeight: `${MAX_ROWS * LINE_HEIGHT_EM}em` }}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <div className="composer-actions">
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
          </div>
        </div>
        {mentions.length > 0 || hint ? (
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
            {hint ? (
              <span className={`composer-hint ${disabledReason ? "error-text" : ""}`} role="status">
                <Icon name={disabledReason ? "alert" : starting ? "loader" : "clock"} size={12} spin={!disabledReason && starting} />
                {hint}
              </span>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
