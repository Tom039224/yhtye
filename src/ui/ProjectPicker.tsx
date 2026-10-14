import { type FocusEvent, type FormEvent, type KeyboardEvent, useEffect, useId, useMemo, useRef, useState } from "react";

import { type HostEntry, type HostKey, type HostProject, LOCAL_HOST, scopedKey as scopedProjectKey } from "../store/hosts";
import { useActiveHost, useAllProjects, useHostsRegistry, useHostsState } from "../store/useHosts";
import { useAppState, useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";
import { projectRing, RING_LABEL } from "./projectActivity";
import { RemoteMark } from "./RemoteMark";

/**
 * Scrolls `list` just far enough to show its `index`th row. Only the list moves: `scrollIntoView`
 * would scroll every box around it as well, up to the page.
 */
function revealRow(list: HTMLElement, index: number): void {
  const row = list.children[index];
  if (!row) return;
  const box = list.getBoundingClientRect();
  const at = row.getBoundingClientRect();
  if (at.top < box.top) list.scrollTop -= box.top - at.top;
  else if (at.bottom > box.bottom) list.scrollTop += at.bottom - box.bottom;
}

/**
 * The names that more than one host has a project of (those rows are told
 * apart by their host's name).
 */
function sharedNames(projects: readonly HostProject[]): ReadonlySet<string> {
  const hostsOf = new Map<string, Set<HostKey>>();
  for (const row of projects) {
    const hosts = hostsOf.get(row.project.name) ?? new Set<HostKey>();
    hosts.add(row.host);
    hostsOf.set(row.project.name, hosts);
  }
  return new Set([...hostsOf].filter(([, hosts]) => hosts.size > 1).map(([name]) => name));
}

/**
 * The project on screen as a pull-down: a listbox of the known projects of
 * every host (this PC's first, then each remote host's; ↑↓ Home End move,
 * Enter / click switch, Esc closes) and the form that opens another
 * repository on the host on screen. A project of another host carries the
 * remote mark after its name (grey while that host is not connected), and its
 * host's name too where another host has a project of the same name. Focus
 * moves into the popup while it is open and back to the trigger when it
 * closes; leaving the popup closes it.
 */
export function ProjectPicker() {
  const store = useStore();
  const registry = useHostsRegistry();
  const own = useAppState((s) => s.projects);
  const everyones = useAllProjects();
  const activeHost = useActiveHost();
  const hostEntries = useHostsState((s) => s.hosts);
  const view = useAppState((s) => s.project);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  // Without a registry (one store is all there is) the list is the store's own.
  const projects = useMemo(
    () => everyones ?? own.map((project): HostProject => ({ host: activeHost, remote: false, hostName: null, project })),
    [everyones, own, activeHost],
  );
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [path, setPath] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const id = useId();
  const popupId = `${id}-popup`;
  const optionId = (i: number) => `${id}-option-${i}`;
  const blocked = !connected || opening;
  const currentId = view?.info.id ?? null;
  const isCurrent = (row: HostProject) => row.host === activeHost && row.project.id === currentId;
  // Another host's projects are as available as that host's connection is.
  const entryOf = (host: HostKey): HostEntry | undefined => hostEntries?.find((h) => h.key === host);
  const isBlocked = (row: HostProject) => (row.host === activeHost ? blocked : entryOf(row.host)?.connection !== "open");
  const shared = useMemo(() => sharedNames(projects), [projects]);
  const current = projects.find(isCurrent)?.project ?? view?.info ?? null;
  const activeEntry = activeHost === LOCAL_HOST ? undefined : entryOf(activeHost);
  const remoteName = activeEntry ? (activeEntry.name ?? activeHost) : null;
  const activeIndex = Math.max(0, Math.min(active, projects.length - 1));

  useEffect(() => {
    if (open) (listRef.current ?? inputRef.current)?.focus();
  }, [open]);

  useEffect(() => {
    if (open && listRef.current) revealRow(listRef.current, activeIndex);
  }, [open, activeIndex]);

  const openPopup = () => {
    setActive(Math.max(0, projects.findIndex(isCurrent)));
    setOpen(true);
  };
  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) triggerRef.current?.focus();
  };
  const choose = (row: HostProject | undefined) => {
    if (!row || isBlocked(row)) return;
    if (!isCurrent(row)) selectRow(row);
    close(true);
  };
  const selectRow = (row: HostProject) => {
    if (registry) void registry.selectProject(row.host, row.project);
    else void store.openProject(row.project.path);
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const target = path.trim();
    if (!target || blocked) return;
    void store.openProject(target);
    setPath("");
    close(true);
  };

  const onTriggerKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (open) listRef.current?.focus();
    else openPopup();
  };
  const onListKeyDown = (e: KeyboardEvent) => {
    const last = projects.length - 1;
    switch (e.key) {
      case "ArrowDown":
        setActive(Math.min(last, activeIndex + 1));
        break;
      case "ArrowUp":
        setActive(Math.max(0, activeIndex - 1));
        break;
      case "Home":
        setActive(0);
        break;
      case "End":
        setActive(last);
        break;
      case "Enter":
      case " ":
        choose(projects[activeIndex]);
        break;
      default:
        return;
    }
    e.preventDefault();
  };
  const onPopupKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    close(true);
  };
  const onBlur = (e: FocusEvent) => {
    if (!rootRef.current?.contains(e.relatedTarget)) setOpen(false);
  };

  const ring = current ? projectRing(current, view) : null;
  const label = current?.name ?? (projects.length > 0 ? "プロジェクトを選択" : "プロジェクトを開く");
  return (
    <div className="project-picker" ref={rootRef} onBlur={onBlur}>
      <button
        type="button"
        ref={triggerRef}
        className="project-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? popupId : undefined}
        title={current ? `${remoteName ? `${remoteName}: ` : ""}${current.path} · ${RING_LABEL[ring ?? "stopped"]}` : undefined}
        onClick={() => (open ? close(false) : openPopup())}
        onKeyDown={onTriggerKeyDown}
      >
        {ring ? <span className={`ring ring-${ring}`} data-testid="current-ring" /> : null}
        <span className={`name ${current ? "" : "placeholder"}`}>{label}</span>
        {current ? <RemoteMark entry={activeEntry} /> : null}
        <Icon name="chevron-down" size={14} className="chevron" />
      </button>
      {open ? (
        <div className="project-popup" id={popupId} tabIndex={-1} onKeyDown={onPopupKeyDown}>
          {projects.length === 0 ? (
            <p className="side-note">まだプロジェクトがありません。</p>
          ) : (
            <ul
              ref={listRef}
              className="project-options"
              role="listbox"
              aria-label="プロジェクト"
              tabIndex={-1}
              aria-activedescendant={optionId(activeIndex)}
              onKeyDown={onListKeyDown}
            >
              {projects.map((row, i) => {
                const p = row.project;
                // A project of another host is not the open one, whose view `view` is.
                const ring = projectRing(p, row.host === activeHost ? view : null);
                const hostName = row.remote ? (row.hostName ?? row.host) : null;
                // Told apart by the host's name only where the name alone would not do.
                const tag = hostName !== null && shared.has(p.name) ? hostName : null;
                return (
                  <li
                    key={scopedProjectKey(row.host, p.id)}
                    id={optionId(i)}
                    role="option"
                    aria-selected={isCurrent(row)}
                    aria-disabled={isBlocked(row) || undefined}
                    className={`project-option ${i === activeIndex ? "active" : ""}`}
                    title={`${hostName ? `${hostName}: ` : ""}${p.path} · ${RING_LABEL[ring]}`}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => choose(row)}
                  >
                    <span className={`ring ring-${ring}`} data-testid={`ring-${p.id}`} />
                    <span className="name">{p.name}</span>
                    <RemoteMark entry={entryOf(row.host)} />
                    {tag ? <span className="host-tag">{tag}</span> : null}
                    <span className="spacer" />
                    {isCurrent(row) ? <Icon name="check" size={14} className="check" /> : null}
                  </li>
                );
              })}
            </ul>
          )}
          <form className="open-form" onSubmit={submit}>
            <input
              ref={inputRef}
              aria-label={remoteName ? `開くリポジトリのパス (${remoteName} 上)` : "開くリポジトリのパス"}
              placeholder={remoteName ? `${remoteName} 上のパス` : "/path/to/git/repository"}
              value={path}
              onChange={(e) => setPath(e.target.value)}
              disabled={blocked}
            />
            <IconButton
              type="submit"
              icon={opening ? "loader" : "arrow-right"}
              spin={opening}
              size={14}
              label="開く"
              title={remoteName ? `${remoteName} 上のリポジトリを開く` : "リポジトリを開く"}
              disabled={blocked || !path.trim()}
            />
          </form>
        </div>
      ) : null}
    </div>
  );
}
