import "./styles/tokens.css";
import "./App.css";

import { useState } from "react";

import { useAppState } from "./store/useStore";
import { ConnectionBanner } from "./ui/ConnectionBanner";
import { IconRail, type View } from "./ui/IconRail";
import { RunsView } from "./ui/RunsView";
import { Sidebar } from "./ui/Sidebar";
import { StatusBar } from "./ui/StatusBar";
import { TitleBar } from "./ui/TitleBar";
import { Workspace } from "./ui/Workspace";

/** The Claude Design layout (docs/design/orchestrator-desktop.md §2), driven by the store. */
function App() {
  const project = useAppState((s) => s.project);
  const [view, setView] = useState<View>("work");
  return (
    <div className="app">
      <TitleBar />
      <ConnectionBanner />
      <div className="body">
        <IconRail view={view} onChange={setView} />
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
    </div>
  );
}

export default App;
