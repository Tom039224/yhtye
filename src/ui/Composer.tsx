import { type KeyboardEvent, useState } from "react";

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
 * The design's composer (§3.4): text, then a row of mention chips, the mode
 * pill and "⏎ 送信". Enter sends, Shift+Enter inserts a newline (IME
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

  const hint =
    disabledReason ??
    (starting
      ? "オーケストレータを起動中…"
      : turnRunning ? "オーケストレータが作業中です。送ったメッセージは待機し、ターンが終わると届きます。" : null);

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
              <span className="at">@</span>
              <span className="label">
                {m.task} {m.title}
              </span>
              <span className="x">×</span>
            </button>
          ))}
          <span className="mode-pill" title="依頼はオーケストレータが受けて配る (他のモードは未定義)">
            orchestrate
          </span>
          <span className="spacer" />
          {turnRunning ? (
            <button type="button" className="btn btn-small btn-danger" onClick={onCancel} disabled={disabled}>
              ターンを中止
            </button>
          ) : null}
          <button type="button" className="send" onClick={() => void send()} disabled={!canSend} title="Enter で送信 · Shift+Enter で改行">
            {sending ? "送信中…" : "⏎ 送信"}
          </button>
          {hint ? <span className={`composer-hint ${disabledReason ? "error-text" : ""}`}>{hint}</span> : null}
        </div>
      </div>
    </div>
  );
}
