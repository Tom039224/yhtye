import "./sidebar.css";

import { useAppState } from "../store/useStore";
import { BackgroundProjects } from "./BackgroundProjects";
import { BranchTree } from "./BranchTree";
import { HostFooter } from "./HostFooter";
import { Icon } from "./Icon";
import { ProjectPicker } from "./ProjectPicker";

/** The open project as a pull-down, the ones running in the background, BRANCHES, and the core host with the paired devices (design §3.3), all from the core. */
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
