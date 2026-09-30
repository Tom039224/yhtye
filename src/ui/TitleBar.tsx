import { selectedChatInfo } from "../store/project";
import { useAppState } from "../store/useStore";

/**
 * Wordmark and the branch pill: the selected chat's branch, or (no chat yet)
 * the branch checked out in the project (from git, not a mock).
 */
export function TitleBar() {
  const branch = useAppState((s) => {
    if (!s.project) return null;
    const chat = selectedChatInfo(s.project);
    if (chat) return chat.branch;
    return s.git && s.git.project === s.project.info.id ? (s.git.overview?.head ?? null) : null;
  });
  return (
    <header className="titlebar">
      <span className="wordmark">Yhtye</span>
      <span className="spacer" />
      {branch ? (
        <span className="branch-pill" title="選択中のチャットの作業対象ブランチ" data-testid="head-branch">
          <span className="dot dot-ok" />
          {branch}
        </span>
      ) : null}
    </header>
  );
}
