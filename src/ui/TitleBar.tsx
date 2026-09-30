import { selectedChatInfo } from "../store/project";
import { useAppState } from "../store/useStore";
import { placeLabel, placeOf } from "./branchTree";
import { Icon } from "./Icon";

/**
 * Wordmark and the branch pill: the branch the selected chat's worktree has
 * checked out now (Stage 8e), or (no chat yet) the branch checked out in the
 * project (from git, not a mock).
 */
export function TitleBar() {
  const branch = useAppState((s) => {
    if (!s.project) return null;
    const overview = s.git && s.git.project === s.project.info.id ? s.git.overview : null;
    const chat = selectedChatInfo(s.project);
    if (chat) return placeLabel(placeOf(chat, overview));
    return overview?.head ?? null;
  });
  return (
    <header className="titlebar">
      <span className="wordmark">Yhtye</span>
      <span className="spacer" />
      {branch ? (
        <span className="branch-pill" title="選択中のチャットの作業ツリーのブランチ" data-testid="head-branch">
          <Icon name="git-branch" size={12} />
          {branch}
        </span>
      ) : null}
    </header>
  );
}
