import {
  type ClipboardEvent,
  type CSSProperties,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useRef,
  useState,
} from "react";

import { attachmentsReady } from "../store/attachments";
import type { DraftAttachment } from "../store/drafts";
import { AttachmentTile, AttachmentViewer } from "./AttachmentTile";
import { mirrorText, useFieldHeight } from "./composerField";
import { Icon, IconButton } from "./Icon";
import type { Mention } from "./mentions";

/** The textarea grows with its text from one row up to this many rows, then scrolls (App.css `.composer-field`). */
const MAX_ROWS = 11;
const FIELD_STYLE = { "--composer-max-rows": MAX_ROWS } as CSSProperties;
const NO_ATTACHMENTS: readonly DraftAttachment[] = [];

interface Props {
  disabled: boolean;
  /** Why sending is not possible (shown instead of the hint). */
  disabledReason: string | null;
  sending: boolean;
  turnRunning: boolean;
  /** Sent, but the orchestrator process is still starting (lazy start). */
  starting?: boolean;
  /** The text the box starts with (the chat's saved draft); the parent remounts the composer per chat. */
  initialText?: string;
  /** Called with the text on every edit and with `""` once a send is accepted (to save or drop the draft). */
  onTextChange?: (text: string) => void;
  mentions: Mention[];
  onRemoveMention: (task: string) => void;
  /** The chat's draft attachments, shown as tiles above the text and sent with it. */
  attachments?: readonly DraftAttachment[];
  /** Files picked with the + button, pasted or dropped (the caller stages them). */
  onAddFiles?: (files: File[]) => void;
  /** The × of a tile (by its draft key). */
  onRemoveAttachment?: (key: string) => void;
  /**
   * Sends `text` (mentions and the staged attachments are added by the
   * caller); resolves to whether it was accepted.
   */
  onSend: (text: string) => Promise<boolean>;
  onCancel: () => void;
  /** Shown below the composer (the context meter and its compact button). */
  footer?: ReactNode;
}

/**
 * The design's composer (§3.4): the text with the + (attach files) button at
 * its lower left and the send button at its lower right (the text uses the full
 * height; the buttons take width only), the attachments as square tiles above
 * it, then a row of mention chips. Files can also be pasted (an image from the
 * clipboard) or dropped on it. Enter sends, Shift+Enter inserts a newline (IME
 * composition is left alone); attachments alone can be sent once all are
 * staged. During an orchestrator turn messages are still accepted: they wait
 * in its inbox and are delivered when the turn ends.
 */
export function Composer({
  disabled,
  disabledReason,
  sending,
  turnRunning,
  starting = false,
  initialText = "",
  onTextChange,
  mentions,
  onRemoveMention,
  attachments = NO_ATTACHMENTS,
  onAddFiles,
  onRemoveAttachment,
  onSend,
  onCancel,
  footer,
}: Props) {
  const [text, setText] = useState(initialText);
  const [dragging, setDragging] = useState(false);
  const [viewing, setViewing] = useState<{ name: string; src: string } | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const ready = attachmentsReady(attachments);
  const canSend = !disabled && !sending && ready && (text.trim().length > 0 || attachments.length > 0);
  const canAttach = !disabled && onAddFiles !== undefined;

  const edit = (next: string) => {
    setText(next);
    onTextChange?.(next);
  };

  // `onTextChange` is the one of the render that sent, so the draft of the chat the text was sent to
  // is dropped even if another chat is shown by the time the send is accepted.
  const send = async () => {
    if (!canSend) return;
    const sent = await onSend(text);
    if (sent) edit("");
  };

  // Wrapped lines do not add rows, so fit the height to the content (capped by CSS `max-height`).
  useFieldHeight(textarea, mirror, text);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void send();
  };

  const addFiles = (files: File[]) => {
    if (canAttach && files.length > 0) onAddFiles(files);
  };

  // An image copied to the clipboard; a copy that also has plain text (a spreadsheet's cells) pastes the text.
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(e.clipboardData.files);
    if (!canAttach || files.length === 0 || e.clipboardData.getData("text/plain") !== "") return;
    e.preventDefault();
    addFiles(files);
  };

  const draggingFiles = (e: DragEvent) => canAttach && Array.from(e.dataTransfer.types).includes("Files");
  const onDragOver = (e: DragEvent<HTMLDivElement>) => {
    if (!draggingFiles(e)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    setDragging(true);
  };
  const onDragLeave = (e: DragEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
  };
  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    setDragging(false);
    if (!draggingFiles(e)) return;
    e.preventDefault();
    addFiles(Array.from(e.dataTransfer.files));
  };

  const uploading = attachments.some((a) => a.status === "uploading");
  const attachHint = ready ? null : uploading ? "添付中… · 終わると送信できます" : "添付できなかったファイルを外すと送信できます";
  const hint = disabledReason ?? (starting ? "起動中…" : (attachHint ?? (turnRunning ? "作業中 · 送信分はターン終了後に届きます" : null)));
  const hintIcon = disabledReason ? "alert" : starting ? "loader" : attachHint ? (uploading ? "loader" : "alert") : "clock";

  return (
    <div className="composer-wrap">
      <div
        className={`composer ${dragging ? "composer-dragging" : ""}`}
        onDragEnter={onDragOver}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
      >
        {attachments.length > 0 ? (
          <div className="composer-attachments" role="list" aria-label="添付ファイル">
            {attachments.map((a) => (
              <AttachmentTile
                key={a.key}
                name={a.name}
                kind={a.kind}
                preview={a.preview}
                delivery={a.meta?.delivery ?? null}
                predicted
                status={a.status}
                error={a.error}
                size={a.meta?.size}
                onRemove={onRemoveAttachment ? () => onRemoveAttachment(a.key) : undefined}
                onOpen={a.preview ? () => setViewing({ name: a.name, src: a.preview ?? "" }) : undefined}
              />
            ))}
          </div>
        ) : null}
        <div className="composer-body">
          {onAddFiles ? (
            <>
              <IconButton
                icon="plus"
                className="composer-attach"
                label="ファイルを添付"
                title="ファイルを添付 · 画像は貼り付け、ファイルはドロップでも添付できます"
                onClick={() => picker.current?.click()}
                disabled={!canAttach}
              />
              <input
                ref={picker}
                type="file"
                multiple
                hidden
                tabIndex={-1}
                aria-hidden="true"
                data-testid="attach-input"
                onChange={(e) => {
                  addFiles(Array.from(e.target.files ?? []));
                  // Picking the same file again must fire `change` again.
                  e.target.value = "";
                }}
              />
            </>
          ) : null}
          <div className="composer-field" style={FIELD_STYLE}>
            <textarea
              ref={textarea}
              aria-label="オーケストレータへのメッセージ"
              placeholder="オーケストレータに指示する…"
              value={text}
              disabled={disabled}
              rows={1}
              onChange={(e) => edit(e.target.value)}
              onKeyDown={onKeyDown}
              onPaste={onPaste}
            />
            <div className="composer-field-mirror" ref={mirror} aria-hidden="true">
              {mirrorText(text)}
            </div>
          </div>
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
                <span className="mention-id">{m.task}</span>
                <span className="label">{m.title}</span>
                <Icon name="x" size={10} className="x" />
              </button>
            ))}
            {hint ? (
              <span className={`composer-hint ${disabledReason ? "error-text" : ""}`} role="status">
                <Icon name={hintIcon} size={12} spin={hintIcon === "loader"} />
                {hint}
              </span>
            ) : null}
          </div>
        ) : null}
      </div>
      {footer}
      {viewing ? <AttachmentViewer name={viewing.name} src={viewing.src} onClose={closeViewer} /> : null}
    </div>
  );
}
