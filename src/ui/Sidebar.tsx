import "./sidebar.css";

import { useAppState, useStore } from "../store/useStore";
import { BranchTree } from "./BranchTree";
import { CurrentProject } from "./CurrentProject";
import { HostFooter } from "./HostFooter";
import { Icon } from "./Icon";

/** The open project's name, BRANCHES, and the core host with the paired devices (design §3.3), all from the core. */
export function Sidebar() {
  const store = useStore();
  const project = useAppState((s) => s.project?.info.id);
  return (
    <aside className="sidebar" aria-label="projects">
      <div className="side-label project-label">
        <Icon name="folder" size={13} />
        Project
      </div>
      <CurrentProject />
      <BranchTree key={project ? store.scopedKey(project) : "none"} />
      <HostFooter />
    </aside>
  );
}
