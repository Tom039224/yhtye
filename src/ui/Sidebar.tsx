import "./sidebar.css";

import { useAppState } from "../store/useStore";
import { BackgroundProjects } from "./BackgroundProjects";
import { BranchTree } from "./BranchTree";
import { Icon } from "./Icon";
import { ProjectPicker } from "./ProjectPicker";
import { useNow } from "./useNow";

/** The open project as a pull-down, the ones running in the background, BRANCHES and the core host (design §3.3), all from the core. */
export function Sidebar() {
  const project = useAppState((s) => s.project?.info.id);
  return (
    <aside className="sidebar" aria-label="projects">
      <ProjectPicker />
      <BackgroundProjects />
      <BranchTree key={project ?? "none"} />
      <HostFooter />
    </aside>
  );
}

/**
 * The design's "remote host" footer. Yhtye runs one local core; this shows the
 * connection to it and when it last sent an event. Remote hosts are not
 * supported (Stage 6b decides whether they come).
 */
function HostFooter() {
  const connection = useAppState((s) => s.connection);
  const transport = useAppState((s) => s.transport);
  const lastEventAt = useAppState((s) => s.lastEventAt);
  const now = useNow(1000);
  const dot = connection.state === "open" ? "dot-ok" : connection.state === "connecting" ? "dot-busy" : "dot-bad";
  const ago = lastEventAt === null ? null : Math.max(0, Math.round((now - lastEventAt) / 1000));
  const kind = transport.kind === "tauri" ? "app" : transport.kind;
  return (
    <div className="side-footer" data-testid="host" title={`ローカルのコア · ${kind}`}>
      <span className={`dot ${dot}`} />
      <span className="host">{kind}</span>
      <span className="spacer" />
      <span className="host-sync" title={ago === null ? "まだイベントを受けていません" : "最後のイベントを受けてから"}>
        <Icon name="clock" size={11} />
        {ago === null ? "—" : formatAgo(ago)}
      </span>
    </div>
  );
}

function formatAgo(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}
