import "./sidebar.css";

import { useAppState } from "../store/useStore";
import { BackgroundProjects } from "./BackgroundProjects";
import { BranchTree } from "./BranchTree";
import { formatPingAge, hostTone, hostToneLabel, pingAgeSeconds } from "./hostStatus";
import { Icon } from "./Icon";
import { ProjectPicker } from "./ProjectPicker";
import { useNow } from "./useNow";

/** The open project as a pull-down, the ones running in the background, BRANCHES and the core host (design §3.3), all from the core. */
export function Sidebar() {
  const project = useAppState((s) => s.project?.info.id);
  return (
    <aside className="sidebar" aria-label="projects">
      <div className="side-label project-label">
        <Icon name="folder" size={13} />
        Project
      </div>
      <ProjectPicker />
      <BackgroundProjects />
      <BranchTree key={project ?? "none"} />
      <HostFooter />
    </aside>
  );
}

/**
 * The machine the core runs on (its host name, read by the core), a dot for the
 * connection to it, and how long ago the core last answered a ping ("now" up
 * to 5 s). Remote hosts would be listed the same way (Stage 6b decides whether
 * they come).
 */
function HostFooter() {
  const connection = useAppState((s) => s.connection.state);
  const host = useAppState((s) => s.host);
  const now = useNow(1000);
  const age = pingAgeSeconds(now, host.lastPingAt);
  const tone = hostTone(connection, age);
  const state = hostToneLabel(tone, connection);
  const name = host.name ?? "—";
  return (
    <div className="side-footer" data-testid="host" title={`${name} · ${state}`}>
      <span className={`dot dot-${tone}`} role="img" aria-label={state} />
      <span className="host">{name}</span>
      <span className="spacer" />
      <span className="host-sync" title={age === null ? "まだ応答がありません" : "最後の ping からの経過"}>
        <Icon name="clock" size={11} />
        {age === null ? "—" : formatPingAge(age)}
      </span>
    </div>
  );
}
