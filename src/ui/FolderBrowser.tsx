import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";

import type { DirectoryListing } from "../api/generated";
import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import { Icon } from "./Icon";

interface FolderBrowserProps {
  /** Where the folders are ("host-name" for a remote host, shown in the heading). */
  hostLabel: string | null;
  /** Opens the folder as a project. Rejects with why not (shown in the browser). */
  onOpen: (path: string) => Promise<void>;
  onClose: () => void;
  /** A project is being opened: the actions wait. */
  busy: boolean;
}

/**
 * Browses the folders of the host on screen (`list_directory`) to pick a
 * project: the OS dialog can only choose on this machine, so this stands in
 * for it on a remote host and in a browser. It lists the folder it is in
 * (starting at the host's home), with a parent entry (↑) and the sub-folders
 * (a git repository is marked); a click enters a folder, "このフォルダを開く"
 * opens the one it is in, and the path field jumps straight to a path. A folder
 * that cannot be listed or opened is explained under the list.
 */
export function FolderBrowser({ hostLabel, onOpen, onClose, busy }: FolderBrowserProps) {
  const store = useStore();
  const [listing, setListing] = useState<DirectoryListing | null>(null);
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [opening, setOpening] = useState(false);
  // Only the newest listing is kept: a slow answer to an earlier click must not replace a later one.
  const latest = useRef(0);

  const go = useCallback(
    async (target?: string) => {
      const mine = ++latest.current;
      setLoading(true);
      setError(null);
      try {
        const next = await store.listDirectory(target);
        if (mine !== latest.current) return;
        setListing(next);
        setPath(next.path);
      } catch (e) {
        if (mine === latest.current) setError(toCommandError(e).message);
      } finally {
        if (mine === latest.current) setLoading(false);
      }
    },
    [store],
  );

  useEffect(() => {
    void go();
  }, [go]);

  const jump = (e: FormEvent) => {
    e.preventDefault();
    const target = path.trim();
    if (target) void go(target);
  };
  const open = async () => {
    if (!listing) return;
    setOpening(true);
    setError(null);
    try {
      await onOpen(listing.path);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setOpening(false);
    }
  };

  return (
    <section className="folder-browser" aria-label="フォルダを選択">
      <h3 className="folder-browser-title">
        <Icon name="folder" size={14} />
        {hostLabel ? `${hostLabel} 上のフォルダを選択` : "フォルダを選択"}
      </h3>
      <form className="folder-path-form" onSubmit={jump}>
        <input aria-label="フォルダのパス" value={path} spellCheck={false} placeholder="/path/to/git/repository" onChange={(e) => setPath(e.target.value)} />
        <button type="submit" className="dash-button" disabled={loading || !path.trim()}>
          移動
        </button>
      </form>
      <div className="folder-current">
        <span className="folder-current-path" data-testid="folder-current" title={listing?.path}>
          {listing?.path ?? "…"}
        </span>
        {listing?.is_git_repo ? (
          <span className="git-chip" title="このフォルダは git リポジトリです">
            <Icon name="git-branch" size={11} />
            git
          </span>
        ) : null}
      </div>
      <ul className="folder-list" aria-label="フォルダ" aria-busy={loading}>
        {listing?.parent != null ? (
          <li>
            <button type="button" className="folder-entry folder-parent" aria-label={`親フォルダ (${listing.parent})`} title={listing.parent} onClick={() => void go(listing.parent ?? undefined)}>
              <Icon name="arrow-up" size={14} />
              <span className="name">..</span>
            </button>
          </li>
        ) : null}
        {listing?.entries.map((entry) => (
          <li key={entry.path}>
            <button
              type="button"
              className="folder-entry"
              aria-label={entry.is_git_repo ? `${entry.name} (git リポジトリ)` : entry.name}
              title={entry.path}
              onClick={() => void go(entry.path)}
            >
              <Icon name="folder" size={14} />
              <span className="name">{entry.name}</span>
              {entry.is_git_repo ? (
                <span className="git-chip">
                  <Icon name="git-branch" size={11} />
                  git
                </span>
              ) : null}
            </button>
          </li>
        ))}
        {listing && listing.entries.length === 0 ? <li className="folder-empty">サブフォルダはありません。</li> : null}
      </ul>
      {error !== null ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
      <div className="folder-actions">
        <button type="button" className="dash-button primary" disabled={!listing || loading || opening || busy} onClick={() => void open()}>
          <Icon name={opening ? "loader" : "check"} size={14} spin={opening} />
          このフォルダを開く
        </button>
        <button type="button" className="dash-button" onClick={onClose}>
          閉じる
        </button>
      </div>
    </section>
  );
}
