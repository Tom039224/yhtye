import { type KeyboardEvent, type MouseEvent, useEffect, useId, useRef, useState } from "react";

import type { ChatInfo } from "../api/generated";
import { toCommandError } from "../api/transport";
import { MAX_CHAT_TITLE_CHARS } from "../store/chats";
import { useAppState, useStore } from "../store/useStore";
import { chatRing, deleteBlocker, shortAge } from "./branchTree";
import { NEW_CHAT_TITLE } from "./Conversation";
import { IconButton } from "./Icon";
import { RowMenu } from "./RowMenu";

type Mode = "view" | "rename" | "delete";
/** Where focus goes when the row is back to its plain state. */
type Return = "row" | "more";

interface ChatRowProps {
  chat: ChatInfo;
  now: number;
  /** Also say where the chat's worktree is (chats whose worktree is gone). */
  showPlace?: boolean;
}

/**
 * One chat of the tree: a button that shows it, and (on hover or focus, where
 * its age is) a "⋯" button whose menu renames it in place or deletes it after
 * a confirmation.
 */
export function ChatRow({ chat, now, showPlace = false }: ChatRowProps) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  const [mode, setMode] = useState<Mode>("view");
  const [menuOpen, setMenuOpen] = useState(false);
  const rowRef = useRef<HTMLButtonElement>(null);
  const moreRef = useRef<HTMLButtonElement>(null);
  const returnTo = useRef<Return | null>(null);
  const menuId = useId();

  useEffect(() => {
    if (mode !== "view" || !returnTo.current) return;
    (returnTo.current === "row" ? rowRef : moreRef).current?.focus();
    returnTo.current = null;
  }, [mode]);

  if (!view) return null;
  const title = chat.title ?? NEW_CHAT_TITLE;
  const back = (to: Return) => {
    returnTo.current = to;
    setMode("view");
  };
  if (mode === "rename") {
    return (
      <li className="chat-item">
        <RenameForm chat={chat} onDone={() => back("row")} />
      </li>
    );
  }
  if (mode === "delete") {
    return (
      <li className="chat-item">
        <DeleteConfirm chat={chat} title={title} onDone={() => back("more")} />
      </li>
    );
  }

  const selected = view.selectedChat === chat.id;
  const ring = chatRing(view, chat.id);
  const openMenu = (e: MouseEvent) => {
    e.preventDefault();
    setMenuOpen(true);
  };
  return (
    <li className={`chat-item ${menuOpen ? "menu-open" : ""}`}>
      <div className="chat-line" onContextMenu={openMenu}>
        <button
          ref={rowRef}
          type="button"
          className={`side-row chat-row ${selected ? "current" : ""}`}
          aria-current={selected ? "true" : undefined}
          title={showPlace ? `${title} · ${chat.worktree}` : title}
          onClick={() => store.selectChat(chat.id)}
        >
          <span className={`ring ring-${ring}`} data-testid={`chat-ring-${chat.id}`} />
          <span className="name">{title}</span>
          {view.unread[chat.id] ? <span className="unread-dot" role="img" aria-label="未読の通知" /> : null}
          <span className="age">{shortAge(now, chat.last_used_ms)}</span>
        </button>
        <IconButton
          ref={moreRef}
          icon="more"
          size={14}
          className="chat-more"
          label={`${title} の操作`}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          aria-controls={menuOpen ? menuId : undefined}
          onClick={() => setMenuOpen((o) => !o)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
            e.preventDefault();
            setMenuOpen(true);
          }}
        />
      </div>
      {menuOpen ? (
        <RowMenu
          id={menuId}
          label={`${title} の操作`}
          anchor={moreRef}
          actions={[
            { id: "rename", label: "名前を変更", icon: "edit", onSelect: () => setMode("rename") },
            { id: "delete", label: "削除", icon: "trash", tone: "danger", onSelect: () => setMode("delete") },
          ]}
          onClose={(refocus) => {
            setMenuOpen(false);
            if (refocus) moreRef.current?.focus();
          }}
        />
      ) : null}
    </li>
  );
}

/** True while an IME is composing (an Enter then confirms the composition, not the form). */
function isComposing(e: KeyboardEvent): boolean {
  return e.nativeEvent.isComposing;
}

/**
 * The chat's title as an input in the row. Enter saves, Esc cancels, leaving
 * the input saves a changed title; a refusal (empty, too long) is shown under it.
 */
function RenameForm({ chat, onDone }: { chat: ChatInfo; onDone: () => void }) {
  const store = useStore();
  const [value, setValue] = useState(chat.title ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Enter and Esc unmount the input, which may report a blur: only the first word counts.
  const settled = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const errorId = useId();

  // Back in the input after a refusal (it is disabled while saving).
  useEffect(() => {
    if (!busy) inputRef.current?.focus();
  }, [busy]);

  const finish = () => {
    settled.current = true;
    onDone();
  };
  const save = async () => {
    if (settled.current || busy) return;
    const title = value.trim();
    if (title === (chat.title ?? "")) return finish();
    if (!title) return setError("名前を入力してください");
    setBusy(true);
    setError(null);
    try {
      await store.renameChat(chat.id, title);
      finish();
    } catch (e) {
      setError(toCommandError(e).message);
      setBusy(false);
    }
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (isComposing(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish();
    }
  };

  return (
    <div className="chat-rename">
      <input
        ref={inputRef}
        aria-label="チャットの名前"
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        placeholder={NEW_CHAT_TITLE}
        maxLength={MAX_CHAT_TITLE_CHARS}
        value={value}
        disabled={busy}
        onChange={(e) => setValue(e.target.value)}
        onFocus={(e) => e.target.select()}
        onKeyDown={onKeyDown}
        onBlur={() => (error ? finish() : void save())}
      />
      {error ? (
        <p id={errorId} className="form-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/**
 * Asks before deleting, in the row: what goes and what stays, then "delete" or
 * "cancel" (focus starts on cancel). If the chat cannot be deleted now it says
 * why instead; a refusal of the core is shown too.
 */
function DeleteConfirm({ chat, title, onDone }: { chat: ChatInfo; title: string; onDone: () => void }) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const messageId = useId();
  const blocker = view ? deleteBlocker(view, chat.id) : null;

  useEffect(() => cancelRef.current?.focus(), []);

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await store.deleteChat(chat.id);
    } catch (e) {
      setError(toCommandError(e).message);
      setBusy(false);
    }
  };
  const shown = blocker ?? error;

  return (
    <div
      className="chat-confirm"
      role="group"
      aria-label="チャットの削除"
      aria-describedby={messageId}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onDone();
        }
      }}
    >
      <p id={messageId} className="chat-confirm-text">
        {blocker ? null : (
          <>
            <span className="chat-confirm-name">{title}</span>
            チャットを削除しますか?
            <span className="chat-confirm-note">会話の記録は消え、作業ツリーとブランチは残ります。</span>
          </>
        )}
      </p>
      {shown ? (
        <p className="form-error" role="alert">
          {shown}
        </p>
      ) : null}
      <div className="chat-confirm-actions">
        {blocker ? null : (
          <IconButton icon="trash" size={14} tone="danger" label="削除する" disabled={busy} onClick={() => void remove()} />
        )}
        <IconButton ref={cancelRef} icon="x" size={14} label={blocker ? "閉じる" : "キャンセル"} disabled={busy} onClick={onDone} />
      </div>
    </div>
  );
}
