import { useId, useRef, useState } from "react";

import { type HostProject, scopedKey } from "../store/hosts";
import { fillSlots } from "../store/order";
import { useAppState, useStore } from "../store/useStore";
import { Icon } from "./Icon";
import { ProjectDot } from "./ProjectDot";
import { ProjectMenu } from "./ProjectMenu";
import { RING_LABEL } from "./projectActivity";
import { RemoteMark } from "./RemoteMark";
import { openedRows, type Projects, useProjects } from "./useProjects";
import { reorderKey, useSortable } from "./useSortable";

/**
 * The "プロジェクト" section of the icon rail: the projects that are open (in
 * any host's core) and the one on screen, one row each. Collapsed, a row is its
 * status dot alone (lit green while work is on in the project, hollow
 * otherwise); the name, the remote mark and the host's name come with the rail
 * expanding (CSS only, they are always in the DOM). A click shows the project
 * (`onShown` then switches to the work view). The project on screen is marked
 * `current`, and `active` (the rail's amber marker) while the work view is the
 * one shown (`working`). A right click opens the menu of a
 * project of the host on screen (sleep, move up / down); those rows can also be
 * dragged onto each other, or moved with Alt + ↑ / ↓. Another host's rows have
 * neither: the screen's store speaks to one host only.
 */
export function RailProjects({ working, onShown }: { working: boolean; onShown: () => void }) {
  const store = useStore();
  const projects = useProjects();
  const own = useAppState((s) => s.projects);
  const connected = useAppState((s) => s.connection.state === "open");
  const shown = openedRows(projects.rows, projects.activeHost, projects.view);
  // Only the host on screen's rows can be moved. They trade the places they have among all that host's projects.
  const ids = shown.filter((row) => row.host === projects.activeHost).map((row) => row.project.id);
  const move = (next: string[]) => {
    const all = own.map((p) => p.id);
    void store.reorderProjects(fillSlots(all, next.filter((id) => all.includes(id))));
  };
  const sortable = useSortable(ids, move, connected);
  return (
    <section className="rail-section rail-projects" aria-label="プロジェクト">
      <h2 className="rail-heading">
        <Icon name="folder" size={13} />
        <span className="rail-label">プロジェクト</span>
      </h2>
      {shown.length === 0 ? <p className="rail-empty rail-label">開いているプロジェクトはありません。</p> : null}
      <ul className="rail-project-list" aria-label="開いているプロジェクト">
        {shown.map((row) => {
          const mine = row.host === projects.activeHost;
          return (
            <li key={scopedKey(row.host, row.project.id)} className="rail-project" {...(mine ? sortable.rowProps(row.project.id) : {})}>
              <RailProjectRow row={row} projects={projects} ids={mine ? ids : null} working={working} onMove={move} onShown={onShown} />
            </li>
          );
        })}
      </ul>
    </section>
  );
}

interface RailProjectRowProps {
  row: HostProject;
  projects: Projects;
  /** The movable rows' ids in order (this row's among them); `null` for a row of another host. */
  ids: string[] | null;
  /** The work view is the one shown. */
  working: boolean;
  onMove: (next: string[]) => void;
  onShown: () => void;
}

function RailProjectRow({ row, projects, ids, working, onMove, onShown }: RailProjectRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const rowRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const p = row.project;
  const ring = projects.ringOf(row);
  const current = projects.isCurrent(row);
  const blocked = projects.isBlocked(row);
  const hostName = row.remote ? (row.hostName ?? row.host) : null;
  return (
    <>
      <button
        ref={rowRef}
        type="button"
        className={`rail-row rail-project-row${current ? " current" : ""}${current && working ? " active" : ""}`}
        title={`${hostName ? `${hostName}: ` : ""}${p.path} · ${RING_LABEL[ring]}`}
        aria-label={`${p.name} に切り替え (${RING_LABEL[ring]})`}
        aria-current={current ? "true" : undefined}
        aria-disabled={blocked || undefined}
        onClick={() => {
          if (blocked) return;
          projects.select(row);
          onShown();
        }}
        onKeyDown={(e) => {
          if (ids) reorderKey(e, ids, p.id, onMove);
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          if (ids) setMenuOpen(true);
        }}
      >
        <span className="rail-icon" aria-hidden="true">
          <ProjectDot ring={ring} testId={`rail-dot-${scopedKey(row.host, p.id)}`} />
        </span>
        <span className="rail-label rail-project-label">
          <span className="name">{p.name}</span>
          <RemoteMark entry={projects.entryOf(row.host)} size={11} />
          {hostName ? <span className="host-tag">{hostName}</span> : null}
        </span>
      </button>
      {menuOpen && ids ? (
        <ProjectMenu
          id={menuId}
          project={p}
          anchor={rowRef}
          order={{ keys: ids, onMove, onMoved: () => rowRef.current?.focus() }}
          onClose={(refocus) => {
            setMenuOpen(false);
            if (refocus) rowRef.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}
