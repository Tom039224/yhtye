import { type FormEvent, type KeyboardEvent, useId, useRef, useState } from "react";

import type { BranchState, BranchStatuses, ChatInfo, GitOverview } from "../api/generated";
import { isInternalBranch } from "../store/chats";
import type { AppState } from "../store/app";
import { toCommandError } from "../api/transport";
import { useAppState, useStore } from "../store/useStore";
import { buildTree, nodeKey, type WorktreeNode } from "./branchTree";
import { ChatRow } from "./ChatRow";
import { Icon, IconButton, type IconName } from "./Icon";
import { RowMenu } from "./RowMenu";
import { useContextMenu } from "./useContextMenu";
import { useNow } from "./useNow";
import { moveActions, reorderKey, type Sortable, useSortable } from "./useSortable";

function selectOverview(s: AppState): GitOverview | null {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git.overview : null;
}

function selectStatuses(s: AppState): BranchStatuses | null {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git.statuses : null;
}

const NONE: string[] = [];
const NO_CHATS: ChatInfo[] = [];

/** The branch order of the open project (`BranchOrderView`). */
function selectOrder(s: AppState): string[] {
  return s.branchOrder && s.project && s.branchOrder.project === s.project.info.id ? s.branchOrder.keys : NONE;
}

/** Chats made after git was last read (`GitView.awaiting`). */
function selectAwaiting(s: AppState): string[] {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git.awaiting : NONE;
}

/**
 * BRANCHES (orchestrator-desktop §9, Stage 8e): the worktrees as a collapsible
 * tree, each named by the branch it has checked out now, with its chats and
 * a button for a new chat; chats whose worktree is gone; the other local
 * branches; "+" opens the new-branch form.
 */
export function BranchTree() {
  const store = useStore();
  // What the tree needs of the project, not the whole view: it renders again for these only.
  const hasProject = useAppState((s) => s.project !== null);
  const chats = useAppState((s) => s.project?.chats ?? NO_CHATS);
  const ready = useAppState((s) => s.project?.phase === "ready" || s.project?.state != null);
  const overview = useAppState(selectOverview);
  const awaiting = useAppState(selectAwaiting);
  const order = useAppState(selectOrder);
  const gitError = useAppState((s) => (s.git && s.project && s.git.project === s.project.info.id ? s.git.error : null));
  const [formOpen, setFormOpen] = useState(false);
  // Explicit open / close per worktree; the default is "open if it has chats or
  // is the main clone" (so its chats are visible in an empty project).
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const now = useNow(30_000);
  const tree = buildTree(chats, overview, awaiting, order);
  const worktreeKeys = tree.worktrees.map(nodeKey);
  const moveWorktrees = (next: string[]) => void store.reorderBranches(next);
  const sortable = useSortable(worktreeKeys, moveWorktrees);
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
          disabled={!hasProject || !ready}
          onClick={() => setFormOpen((o) => !o)}
        />
      </div>
      {formOpen && hasProject ? <NewBranchForm overview={overview} onClose={() => setFormOpen(false)} /> : null}
      {!hasProject ? <p className="side-note">プロジェクトを開くと表示します。</p> : null}
      {hasProject && !overview && gitError ? <p className="side-note error-text">{gitError}</p> : null}
      {hasProject && empty ? <p className="side-note">作業ツリーはまだありません。</p> : null}
      {hasProject ? (
        <ul className="side-list branch-tree" aria-label="branches">
          {tree.worktrees.map((w) => (
            <WorktreeRow
              key={w.path}
              node={w}
              open={isOpen(w)}
              now={now}
              keys={worktreeKeys}
              sortable={sortable}
              onMove={moveWorktrees}
              onToggle={() => toggle(w)}
              onNewChat={() => void store.createChat({ worktree: w.path })}
            />
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
  /** The worktrees on screen, in order (their reorder is dragged, or done from the menu / Alt + ↑ ↓). */
  keys: string[];
  sortable: Sortable;
  onMove: (next: string[]) => void;
  onToggle: () => void;
  onNewChat: () => void;
}

/**
 * The icon and tone of each PR / merge state, after Orca's sidebar: a plain
 * branch until there is a PR, the PR glyph in green while it is open, the merge
 * glyph in purple once merged (with a badge when new work came after the merge),
 * and a question mark when the state could not be read.
 */
const STATUS_ICON: Record<BranchState, { icon: IconName; tone: string }> = {
  unmerged: { icon: "git-branch", tone: "branch-status-unmerged" },
  pr_open: { icon: "git-pull-request", tone: "branch-status-open" },
  merged: { icon: "git-merge", tone: "branch-status-merged" },
  merged_with_changes: { icon: "git-merge", tone: "branch-status-changes" },
  unknown: { icon: "help", tone: "branch-status-unknown" },
};

const STATUS_ICON_SIZE = 13;

/** Japanese label of a branch's status icon, with the PR number when there is one. */
export function statusLabel(state: BranchState, pr: number | null): string {
  switch (state) {
    case "unmerged":
      return "まだ取り込まれていません";
    case "pr_open":
      return pr == null ? "PR 作成済み" : `PR 作成済み (#${pr})`;
    case "merged":
      return pr == null ? "取り込み済み" : `PR でマージ済み (#${pr})`;
    case "merged_with_changes":
      return pr == null ? "取り込み済み・未マージの変更あり" : `PR でマージ済み・未マージの変更あり (#${pr})`;
    case "unknown":
      return "状態不明";
  }
}

/**
 * The icon left of a branch: its PR / merge status. The default branch itself
 * and a detached worktree get the neutral branch icon (no state, no label).
 */
function StatusIcon({ branch, statuses }: { branch: string | null; statuses: BranchStatuses | null }) {
  if (branch == null || branch === statuses?.default_branch) {
    return (
      <span className="branch-status">
        <Icon name="git-branch" size={STATUS_ICON_SIZE} />
      </span>
    );
  }
  const found = statuses?.statuses.find((s) => s.branch === branch);
  const state = found?.state ?? "unknown";
  const label = statusLabel(state, found?.pr_number ?? null);
  const { icon, tone } = STATUS_ICON[state];
  return (
    <span className={`branch-status ${tone}`} role="img" aria-label={label} title={label}>
      <Icon name={icon} size={STATUS_ICON_SIZE} />
      {state === "merged_with_changes" ? <span className="branch-status-badge" /> : null}
    </span>
  );
}

function WorktreeRow({ node, open, now, keys, sortable, onMove, onToggle, onNewChat }: WorktreeRowProps) {
  const menu = useContextMenu();
  const menuId = useId();
  const rowRef = useRef<HTMLButtonElement>(null);
  const overview = useAppState(selectOverview);
  const statuses = useAppState(selectStatuses);
  const branch = overview?.worktrees.find((w) => w.path === node.path)?.branch ?? null;
  const where = node.isMain ? `${node.path} · メインクローン` : node.path;
  return (
    <li>
      <div className="wt-head" {...sortable.rowProps(node.path)} onContextMenu={menu.onContextMenu}>
        <button
          ref={rowRef}
          type="button"
          className="side-row branch-row"
          aria-expanded={open}
          title={where}
          onClick={onToggle}
          onKeyDown={(e) => void reorderKey(e, keys, node.path, onMove)}
        >
          <Icon name="chevron-down" size={12} className={`caret ${open ? "" : "collapsed"}`} />
          <StatusIcon branch={branch} statuses={statuses} />
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
      {menu.isOpen ? (
        <RowMenu
          id={menuId}
          label={`${node.label} の操作`}
          anchor={menu.anchor}
          actions={moveActions(keys, node.path, onMove)}
          onClose={(refocus) => {
            menu.close();
            if (refocus) rowRef.current?.focus();
          }}
        />
      ) : null}
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
  const moveBranches = (next: string[]) => void store.reorderBranches(next);
  const sortable = useSortable(names, moveBranches);
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
            <OtherBranchRow key={n} name={n} names={names} sortable={sortable} onMove={moveBranches} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

interface OtherBranchRowProps {
  name: string;
  /** The branches on screen, in order. */
  names: string[];
  sortable: Sortable;
  onMove: (next: string[]) => void;
}

function OtherBranchRow({ name, names, sortable, onMove }: OtherBranchRowProps) {
  const store = useStore();
  const statuses = useAppState(selectStatuses);
  const menu = useContextMenu();
  const menuId = useId();
  const rowRef = useRef<HTMLButtonElement>(null);
  return (
    <li {...sortable.rowProps(name)} onContextMenu={menu.onContextMenu}>
      <button
        ref={rowRef}
        type="button"
        className="side-row new-chat"
        title={`${name} の作業ツリーを作ってチャットを始める`}
        aria-label={`${name} の新しいチャット`}
        onClick={() => void store.createChat({ branch: name })}
        onKeyDown={(e) => void reorderKey(e, names, name, onMove)}
      >
        <StatusIcon branch={name} statuses={statuses} />
        <Icon name="message-plus" size={13} />
        <span className="name">{name}</span>
      </button>
      {menu.isOpen ? (
        <RowMenu
          id={menuId}
          label={`${name} の操作`}
          anchor={menu.anchor}
          actions={moveActions(names, name, onMove)}
          onClose={(refocus) => {
            menu.close();
            if (refocus) rowRef.current?.focus();
          }}
        />
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
