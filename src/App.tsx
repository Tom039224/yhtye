import "./styles/tokens.css";
import "./App.css";

import { useCallback, useState } from "react";

import { useAppState } from "./store/useStore";
import { ConnectionBanner } from "./ui/ConnectionBanner";
import { IconRail, type View } from "./ui/IconRail";
import { RunsView } from "./ui/RunsView";
import { SettingsModal } from "./ui/SettingsModal";
import { Sidebar } from "./ui/Sidebar";
import { StatusBar } from "./ui/StatusBar";
import { TitleBar } from "./ui/TitleBar";
import { Workspace } from "./ui/Workspace";

/** The Claude Design layout (docs/design/orchestrator-desktop.md §2), driven by the store. */
function App() {
  const project = useAppState((s) => s.project);
  const [view, setView] = useState<View>("work");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  return (
    <div className="app">
      <TitleBar />
      <ConnectionBanner />
      <div className="body">
        <IconRail view={view} onChange={setView} settingsOpen={settingsOpen} onOpenSettings={() => setSettingsOpen(true)} />
        {view === "runs" ? (
          <RunsView />
        ) : (
          <>
            <Sidebar />
            {project ? (
              <Workspace key={project.info.id} view={project} />
            ) : (
              <div className="main-empty">
                <p className="empty">プロジェクト (git リポジトリ) を開いてください。</p>
              </div>
            )}
          </>
        )}
      </div>
      <StatusBar />
      {settingsOpen ? (
        <SettingsModal project={project ? { id: project.info.id, name: project.info.name } : null} onClose={closeSettings} />
      ) : null}
    </div>
  );
}

export default App;
