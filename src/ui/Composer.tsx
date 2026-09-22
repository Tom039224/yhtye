import { type KeyboardEvent, useState } from "react";

interface Props {
  disabled: boolean;
  /** Why sending is not possible (shown instead of the hint). */
  disabledReason: string | null;
  sending: boolean;
  turnRunning: boolean;
  onSend: (text: string) => Promise<boolean>;
  onCancel: () => void;
}

/**
 * Enter sends, Shift+Enter inserts a newline (IME composition is left alone).
 * During an orchestrator turn messages are still accepted: they are queued in
 * its inbox and delivered when the turn ends.
 */
export function Composer({ disabled, disabledReason, sending, turnRunning, onSend, onCancel }: Props) {
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

  const hint = disabledReason
    ?? (turnRunning ? "オーケストレータが作業中です。送ったメッセージは待機し、ターンが終わると届きます。" : "Enter で送信 · Shift+Enter で改行");

  return (
    <div className="composer">
      <textarea
        aria-label="オーケストレータへのメッセージ"
        placeholder="オーケストレータに指示する…"
        value={text}
        disabled={disabled}
        rows={3}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="composer-row">
        <span className={`composer-hint ${disabledReason ? "error-text" : ""}`}>{hint}</span>
        {turnRunning ? (
          <button type="button" className="btn btn-danger" onClick={onCancel} disabled={disabled}>
            ターンを中止
          </button>
        ) : null}
        <button type="button" className="btn" onClick={() => void send()} disabled={!canSend}>
          {sending ? "送信中…" : "⏎ 送信"}
        </button>
      </div>
    </div>
  );
}
