import { type HostProject, scopedKey } from "../store/hosts";
import { AddProject } from "./AddProject";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { ProjectDot } from "./ProjectDot";
import { RING_LABEL } from "./projectActivity";
import { RemoteMark } from "./RemoteMark";
import { type Projects, useProjects } from "./useProjects";

/**
 * The dashboard's project list: every registered project of every host (in the
 * order the hosts give them, most recently used first), each openable with a
 * click, which also switches to the work view. A project of another host
 * carries the remote mark and its host's name. "プロジェクトを追加" is above.
 */
export function DashboardProjects({ onOpenWork }: { onOpenWork: () => void }) {
  const projects = useProjects();
  return (
    <section className="dash-section dash-projects" aria-label="プロジェクト">
      <header className="dash-section-head">
        <h2 className="dash-heading">
          <Icon name="folder" size={15} />
          プロジェクト
        </h2>
      </header>
      <AddProject onOpened={onOpenWork} />
      {projects.rows.length === 0 ? (
        <EmptyState icon="folder" text="まだプロジェクトがありません。「プロジェクトを追加」から git リポジトリを選んでください。" />
      ) : (
        <ul className="dash-project-list" aria-label="登録済みのプロジェクト">
          {projects.rows.map((row) => (
            <li key={scopedKey(row.host, row.project.id)}>
              <DashboardProject row={row} projects={projects} onOpenWork={onOpenWork} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function DashboardProject({ row, projects, onOpenWork }: { row: HostProject; projects: Projects; onOpenWork: () => void }) {
  const p = row.project;
  const ring = projects.ringOf(row);
  const blocked = projects.isBlocked(row);
  const hostName = row.remote ? (row.hostName ?? row.host) : null;
  return (
    <button
      type="button"
      className={`dash-project${projects.isCurrent(row) ? " current" : ""}`}
      title={`${hostName ? `${hostName}: ` : ""}${p.path}`}
      aria-label={`${p.name} を開く (${RING_LABEL[ring]})`}
      aria-current={projects.isCurrent(row) ? "true" : undefined}
      aria-disabled={blocked || undefined}
      onClick={() => {
        if (blocked) return;
        projects.select(row);
        onOpenWork();
      }}
    >
      <ProjectDot ring={ring} testId={`dash-dot-${scopedKey(row.host, p.id)}`} />
      <span className="dash-project-main">
        <span className="dash-project-name">
          <span className="name">{p.name}</span>
          <RemoteMark entry={projects.entryOf(row.host)} />
          {hostName ? <span className="host-tag">{hostName}</span> : null}
        </span>
        <span className="dash-project-path">{p.path}</span>
      </span>
      <span className="dash-project-state">{RING_LABEL[ring]}</span>
    </button>
  );
}
