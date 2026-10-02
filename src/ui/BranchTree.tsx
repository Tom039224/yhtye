import { type FormEvent, type KeyboardEvent, useState } from "react";

import type { GitOverview } from "../api/generated";
import { isInternalBranch } from "../store/chats";
import type { AppState } from "../store/app";
import { toCommandError } from "../api/transport";
import { useAppState, useStore } from "../store/useStore";
import { buildTree, chatRing, type WorktreeNode } from "./branchTree";
import { ChatRow } from "./ChatRow";
import { Icon, IconButton } from "./Icon";
import { useNow } from "./useNow";

function selectOverview(s: AppState): GitOverview | null {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git.overview : null;
}

/**
 * BRANCHES (orchestrator-desktop §9, Stage 8e): the worktrees as a collapsible
 * tree, each named by the branch it has checked out now, with its chats and
 * a button for a new chat; chats whose worktree is gone; the other local
 * branches; "+" opens the new-branch form.
 */
export function BranchTree() {
  const store = useStore();
  const view = useAppState((s) => s.project);
  const overview = useAppState(selectOverview);
  const gitError = useAppState((s) => (s.git && s.project && s.git.project === s.project.info.id ? s.git.error : null));
  const [formOpen, setFormOpen] = useState(false);
  // Explicit open / close per worktree; the default is "open if it has chats or
  // is the main clone" (so its chats are visible in an empty project).
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const now = useNow(30_000);
  const ready = view?.phase === "ready" || view?.state != null;
  const tree = buildTree(view?.chats ?? [], overview);
  const isOpen = (w: WorktreeNode) => toggled[w.path] ?? (w.chats.length > 0 || w.isMain);
  const toggle = (w: WorktreeNode) => setToggled((t) => ({ ...t, [w.path]: !isOpen(w) }));
  const empty = tree.worktrees.length === 0 && tree.lost.length === 0 && tree.otherBranches.length === 0;

  return (
    <div className="branches">
      <div className="section-title branches-title section-head">
        <span className="side-label">
          <Icon name="git-branch" size={13} />
          Branch
        </span>
        <IconButton
          icon="plus"
          size={14}
          label="新しいブランチ"
          aria-expanded={formOpen}
          disabled={!view || !ready}
          onClick={() => setFormOpen((o) => !o)}
        />
      </div>
      {formOpen && view ? <NewBranchForm overview={overview} onClose={() => setFormOpen(false)} /> : null}
      {!view ? <p className="side-note">プロジェクトを開くと表示します。</p> : null}
      {view && !overview && gitError ? <p className="side-note error-text">{gitError}</p> : null}
      {view && empty ? <p className="side-note">作業ツリーはまだありません。</p> : null}
      {view ? (
        <ul className="side-list branch-tree" aria-label="branches">
          {tree.worktrees.map((w) => (
            <WorktreeRow key={w.path} node={w} open={isOpen(w)} now={now} onToggle={() => toggle(w)} onNewChat={() => void store.createChat({ worktree: w.path })} />
          ))}
          {tree.lost.length > 0 ? (
            <li className="deleted-branches">
              <div className="side-note lost-title" title="見つからない作業ツリーのチャット">
                <Icon name="alert" size={12} />
                <span className="sr-only">(見つからない作業ツリー)</span>
              </div>
              <ul className="chat-list">
                {tree.lost.map((c) => (
                  <ChatRow key={c.id} chat={c} now={now} showPlace />
                ))}
              </ul>
            </li>
          ) : null}
          {tree.otherBranches.length > 0 ? <OtherBranches names={tree.otherBranches} /> : null}
        </ul>
      ) : null}
    </div>
  );
}

interface WorktreeRowProps {
  node: WorktreeNode;
  open: boolean;
  now: number;
  onToggle: () => void;
  onNewChat: () => void;
}

function WorktreeRow({ node, open, now, onToggle, onNewChat }: WorktreeRowProps) {
  const view = useAppState((s) => s.project);
  const running = Boolean(view && node.chats.some((c) => chatRing(view, c.id) === "running"));
  const where = node.isMain ? `${node.path} · メインクローン` : node.path;
  return (
    <li>
      <div className="wt-head">
        <button type="button" className="side-row branch-row" aria-expanded={open} title={where} onClick={onToggle}>
          <Icon name="chevron-down" size={12} className={`caret ${open ? "" : "collapsed"}`} />
          {running ? <span className="ring ring-running" role="img" aria-label="実行中" /> : <span className={`dot ${node.isMain ? "dot-ok" : ""}`} />}
          <span className="name">{node.label}</span>
          {node.missing ? (
            <span className="age" title="作業ツリーが見つかりません">
              <Icon name="alert" size={12} />
              <span className="sr-only">見つかりません</span>
            </span>
          ) : null}
        </button>
        {node.missing ? null : (
          <IconButton icon="message-plus" size={14} className="row-action" label={`${node.label} の新しいチャット`} onClick={onNewChat} />
        )}
      </div>
      {open ? (
        <ul className="chat-list" aria-label={`chats of ${node.label}`}>
          {node.chats.map((c) => (
            <ChatRow key={c.id} chat={c} now={now} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/** Local branches no worktree has checked out; a chat there gets a Yhtye worktree. */
function OtherBranches({ names }: { names: string[] }) {
  const store = useStore();
  const [open, setOpen] = useState(false);
  return (
    <li>
      <button
        type="button"
        className="side-row branch-row other-branches"
        aria-expanded={open}
        aria-label={`他のブランチ (${names.length})`}
        title="作業ツリーのない他のブランチ"
        onClick={() => setOpen((o) => !o)}
      >
        <Icon name="chevron-down" size={12} className={`caret ${open ? "" : "collapsed"}`} />
        <Icon name="git-branch" size={13} />
        <span className="name">{names.length}</span>
      </button>
      {open ? (
        <ul className="chat-list" aria-label="other branches">
          {names.map((n) => (
            <li key={n}>
              <button type="button" className="side-row new-chat" title={`${n} の作業ツリーを作ってチャットを始める`} onClick={() => void store.createChat({ branch: n })} aria-label={`${n} の新しいチャット`}>
                <Icon name="message-plus" size={13} />
                <span className="name">{n}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
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
      <IconButton type="submit" icon={busy ? "loader" : "check"} spin={busy} size={14} label="作成" disabled={busy || !name.trim()} />
      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
    </form>
  );
}
