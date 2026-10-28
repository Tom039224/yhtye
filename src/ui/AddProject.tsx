import { useState } from "react";

import { canPickLocalFolder, pickLocalFolder } from "../api/folderPicker";
import { toCommandError } from "../api/transport";
import { useActiveHostEntry } from "../store/useHosts";
import { useAppState, useStore } from "../store/useStore";
import { FolderBrowser } from "./FolderBrowser";
import { Icon } from "./Icon";

/**
 * "プロジェクトを追加" (the dashboard): opens a git repository as a project on
 * the host on screen. On this PC inside the app that is the OS folder dialog;
 * on a remote host (whose folders the dialog cannot reach) and in a browser it
 * is the {@link FolderBrowser}, which lists the host's own folders. A failure
 * to pick or to open is shown under the button. Once the project is open,
 * `onOpened` switches to the work view.
 */
export function AddProject({ onOpened }: { onOpened: () => void }) {
  const store = useStore();
  const entry = useActiveHostEntry();
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const [browsing, setBrowsing] = useState(false);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const remote = entry?.remote ?? false;
  const hostName = remote ? (entry?.name ?? entry?.key ?? null) : null;
  const native = canPickLocalFolder(remote);

  const openPath = async (path: string) => {
    await store.tryOpenProject(path);
    setBrowsing(false);
    onOpened();
  };
  const pickNative = async () => {
    setPicking(true);
    try {
      const path = await pickLocalFolder("プロジェクトのフォルダを選択");
      if (path !== null) await openPath(path);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setPicking(false);
    }
  };
  const start = () => {
    setError(null);
    if (native) void pickNative();
    else setBrowsing((open) => !open);
  };

  return (
    <div className="add-project">
      <div className="add-project-bar">
        <button
          type="button"
          className="dash-button primary add-project-button"
          aria-expanded={native ? undefined : browsing}
          disabled={!connected || opening || picking}
          title={hostName ? `${hostName} 上のフォルダを選んで追加` : "フォルダを選んで追加"}
          onClick={start}
        >
          <Icon name={picking ? "loader" : "plus"} size={14} spin={picking} />
          プロジェクトを追加
        </button>
        {hostName ? <span className="add-project-target">追加先: {hostName}</span> : null}
      </div>
      {error !== null ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
      {browsing && !native ? <FolderBrowser hostLabel={hostName} busy={opening} onOpen={openPath} onClose={() => setBrowsing(false)} /> : null}
    </div>
  );
}
