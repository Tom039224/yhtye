import "./styles/tokens.css";
import "./App.css";

import { useAppState } from "./store/useStore";
import { ConnectionBanner } from "./ui/ConnectionBanner";
import { ProjectPicker } from "./ui/ProjectPicker";
import { Workspace } from "./ui/Workspace";

/**
 * Plain functional UI (Stage 4): design tokens only; the Claude Design layout
 * (docs/design/orchestrator-desktop.md) is applied in Stage 6.
 */
function App() {
  const view = useAppState((s) => s.project);
  return (
    <div className="app">
      <header className="titlebar">
        <span className="wordmark">Yhtye</span>
      </header>
      <ConnectionBanner />
      <div className="main">
        <ProjectPicker />
        {view ? (
          <Workspace key={view.info.id} view={view} />
        ) : (
          <p className="empty main-empty">プロジェクト (git リポジトリ) を開いてください。</p>
        )}
      </div>
    </div>
  );
}

export default App;
