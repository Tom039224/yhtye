import "./styles/tokens.css";
import "./App.css";

import { useCallback, useEffect, useRef, useState } from "react";

import { useHostsRegistry } from "./store/useHosts";
import { useAppState, useStore } from "./store/useStore";
import { ConnectionBanner } from "./ui/ConnectionBanner";
import { EmptyState } from "./ui/EmptyState";
import { IconRail, type View } from "./ui/IconRail";
import { sidebarBounds } from "./ui/layout";
import { PairLinkDialog } from "./ui/PairLinkDialog";
import { Resizer } from "./ui/Resizer";
import { RunsView } from "./ui/RunsView";
import { SettingsModal } from "./ui/SettingsModal";
import { Sidebar } from "./ui/Sidebar";
import { StatusBar } from "./ui/StatusBar";
import { TitleBar } from "./ui/TitleBar";
import { setSizeVar, sizeVar, useLayoutSizes } from "./ui/useLayoutSizes";
import { Workspace } from "./ui/Workspace";

const SIDEBAR_WIDTH = "--sidebar-width";

/**
 * Re-reads git when the window gets the focus: a branch renamed or switched
 * outside Yhtye shows up in the tree (Stage 8e: names are read, not stored).
 */
function useGitOnFocus(): void {
  const store = useStore();
  useEffect(() => {
    const onFocus = () => void store.refreshGit();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [store]);
}

/** The Claude Design layout (docs/design/orchestrator-desktop.md §2), driven by the store. */
function App() {
  const store = useStore();
  const hosts = useHostsRegistry();
  const project = useAppState((s) => s.project);
  useGitOnFocus();
  const [view, setView] = useState<View>("work");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const { sizes, setSize } = useLayoutSizes();
  const body = useRef<HTMLDivElement>(null);
  return (
    <div className="app">
      <TitleBar />
      <ConnectionBanner />
      <div className="body" ref={body} style={sizeVar(SIDEBAR_WIDTH, sizes.sidebar)}>
        <IconRail view={view} onChange={setView} settingsOpen={settingsOpen} onOpenSettings={() => setSettingsOpen(true)} />
        {view === "runs" ? (
          <RunsView />
        ) : (
          <>
            <Sidebar />
            <Resizer
              orientation="vertical"
              side="before"
              label="サイドバーの幅"
              measure={() => body.current?.querySelector(":scope > .sidebar")?.getBoundingClientRect().width ?? 0}
              bounds={() => sidebarBounds(body.current?.clientWidth)}
              onResize={(px) => setSizeVar(body.current, SIDEBAR_WIDTH, px)}
              onCommit={(px) => setSize("sidebar", px)}
            />
            {project ? (
              <Workspace key={store.scopedKey(project.info.id)} view={project} />
            ) : (
              <div className="main-empty">
                <EmptyState icon="folder" text="プロジェクト (git リポジトリ) を開いてください。" />
              </div>
            )}
          </>
        )}
      </div>
      <StatusBar />
      {hosts ? <PairLinkDialog registry={hosts} /> : null}
      {settingsOpen ? (
        <SettingsModal project={project ? { id: project.info.id, name: project.info.name } : null} onClose={closeSettings} />
      ) : null}
    </div>
  );
}

export default App;
