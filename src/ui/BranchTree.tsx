import { type FormEvent, type KeyboardEvent, useState } from "react";

import type { ChatInfo, GitOverview } from "../api/generated";
import { isInternalBranch } from "../store/chats";
import type { AppState } from "../store/app";
import { toCommandError } from "../api/transport";
import { useAppState, useStore } from "../store/useStore";
import { type BranchNode, buildTree, chatRing, shortAge } from "./branchTree";
import { NEW_CHAT_TITLE } from "./Conversation";
import { useNow } from "./useNow";

function selectOverview(s: AppState): GitOverview | null {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git.overview : null;
}

/**
 * BRANCHES (orchestrator-desktop §9): local branches as a collapsible tree,
 * each with its chats and "+ 新しいチャット"; "+" opens the new-branch form.
 */
export function BranchTree() {
  const store = useStore();
  const view = useAppState((s) => s.project);
  const overview = useAppState(selectOverview);
  const gitError = useAppState((s) => (s.git && s.project && s.git.project === s.project.info.id ? s.git.error : null));
  const [formOpen, setFormOpen] = useState(false);
  // Explicit open / close per branch; the default is "open if it has chats or is
  // checked out in the main clone" (so + 新しいチャット is visible in an empty project).
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const now = useNow(30_000);
  const ready = view?.phase === "ready" || view?.state != null;
  const tree = buildTree(view?.chats ?? [], overview);
  const isOpen = (b: BranchNode) => toggled[b.name] ?? (b.chats.length > 0 || b.isHead);
  const toggle = (b: BranchNode) => setToggled((t) => ({ ...t, [b.name]: !isOpen(b) }));

  return (
    <>
      <div className="section-title branches-title section-head">
        <span>BRANCHES</span>
        <button
          type="button"
          className="icon-btn"
          aria-label="新しいブランチ"
          title="新しいブランチ"
          aria-expanded={formOpen}
          disabled={!view || !ready}
          onClick={() => setFormOpen((o) => !o)}
        >
          +
        </button>
      </div>
      {formOpen && view ? <NewBranchForm overview={overview} onClose={() => setFormOpen(false)} /> : null}
      {!view ? <p className="side-note">プロジェクトを開くと表示します。</p> : null}
      {view && !overview && gitError ? <p className="side-note error-text">{gitError}</p> : null}
      {view && tree.branches.length === 0 && tree.deleted.length === 0 ? <p className="side-note">ブランチはまだありません。</p> : null}
      {view ? (
        <ul className="side-list branch-tree" aria-label="branches">
          {tree.branches.map((b) => (
            <BranchRow key={b.name} node={b} open={isOpen(b)} now={now} onToggle={() => toggle(b)} onNewChat={() => void store.createChat(b.name)} />
          ))}
          {tree.deleted.length > 0 ? (
            <li className="deleted-branches">
              <div className="side-note">(削除されたブランチ)</div>
              <ul className="chat-list">
                {tree.deleted.map((c) => (
                  <ChatRow key={c.id} chat={c} now={now} showBranch />
                ))}
              </ul>
            </li>
          ) : null}
        </ul>
      ) : null}
    </>
  );
}

interface BranchRowProps {
  node: BranchNode;
  open: boolean;
  now: number;
  onToggle: () => void;
  onNewChat: () => void;
}

function BranchRow({ node, open, now, onToggle, onNewChat }: BranchRowProps) {
  const view = useAppState((s) => s.project);
  const running = Boolean(view && node.chats.some((c) => chatRing(view, c.id) === "running"));
  return (
    <li>
      <button
        type="button"
        className="side-row branch-row"
        aria-expanded={open}
        title={node.isHead ? `${node.name} · メイン作業ツリーで checkout 中` : node.name}
        onClick={onToggle}
      >
        <span className="caret" aria-hidden="true">{open ? "▾" : "▸"}</span>
        {running ? <span className="ring ring-running" role="img" aria-label="実行中" /> : <span className={`dot ${node.isHead ? "dot-ok" : ""}`} />}
        <span className="name">{node.name}</span>
      </button>
      {open ? (
        <ul className="chat-list" aria-label={`chats of ${node.name}`}>
          {node.chats.map((c) => (
            <ChatRow key={c.id} chat={c} now={now} />
          ))}
          <li>
            <button type="button" className="side-row new-chat" onClick={onNewChat} aria-label={`${node.name} の新しいチャット`}>
              + 新しいチャット
            </button>
          </li>
        </ul>
      ) : null}
    </li>
  );
}

function ChatRow({ chat, now, showBranch = false }: { chat: ChatInfo; now: number; showBranch?: boolean }) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  if (!view) return null;
  const selected = view.selectedChat === chat.id;
  const ring = chatRing(view, chat.id);
  const title = chat.title ?? NEW_CHAT_TITLE;
  return (
    <li>
      <button
        type="button"
        className={`side-row chat-row ${selected ? "current" : ""}`}
        aria-current={selected ? "true" : undefined}
        title={showBranch ? `${title} · ${chat.branch}` : title}
        onClick={() => store.selectChat(chat.id)}
      >
        <span className={`ring ring-${ring}`} data-testid={`chat-ring-${chat.id}`} />
        <span className="name">{title}</span>
        {view.unread[chat.id] ? <span className="unread-dot" role="img" aria-label="未読の通知" /> : null}
        <span className="age">{shortAge(now, chat.last_used_ms)}</span>
      </button>
    </li>
  );
}

const HEAD_OPTION = "";

/** Name + start point; Enter creates, Esc closes, a refusal is shown in the form. */
function NewBranchForm({ overview, onClose }: { overview: GitOverview | null; onClose: () => void }) {
  const store = useStore();
  const [name, setName] = useState("");
  const [from, setFrom] = useState(HEAD_OPTION);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const locals = (overview?.branches ?? []).map((b) => b.name).filter((n) => !isInternalBranch(n));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await store.createBranch(name.trim(), from === HEAD_OPTION ? undefined : from);
      onClose();
    } catch (err) {
      setError(toCommandError(err).message);
      setBusy(false);
    }
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") onClose();
  };

  return (
    <form className="branch-form" aria-label="新しいブランチ" onSubmit={(e) => void submit(e)} onKeyDown={onKeyDown}>
      <input
        aria-label="ブランチ名"
        placeholder="feature/name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        disabled={busy}
        autoFocus
      />
      <select aria-label="開始点" value={from} onChange={(e) => setFrom(e.target.value)} disabled={busy}>
        <option value={HEAD_OPTION}>HEAD ({overview?.head ?? "detached"})</option>
        {locals.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
      <button type="submit" className="btn btn-small" disabled={busy || !name.trim()}>
        {busy ? "…" : "作成"}
      </button>
      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
    </form>
  );
}
