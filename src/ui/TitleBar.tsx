import { useAppState } from "../store/useStore";

/** Wordmark and the open project's checked-out branch (from git, not a mock). */
export function TitleBar() {
  const head = useAppState((s) => (s.git && s.project && s.git.project === s.project.info.id ? s.git.overview?.head : null));
  return (
    <header className="titlebar">
      <span className="wordmark">Yhtye</span>
      <span className="spacer" />
      {head ? (
        <span className="branch-pill" title="プロジェクトで checkout されているブランチ" data-testid="head-branch">
          <span className="dot dot-ok" />
          {head}
        </span>
      ) : null}
    </header>
  );
}
