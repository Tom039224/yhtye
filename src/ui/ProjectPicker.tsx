import { type FocusEvent, type FormEvent, type KeyboardEvent, useEffect, useId, useRef, useState } from "react";

import type { ProjectInfo } from "../api/generated";
import { useAppState, useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";
import { projectRing, RING_LABEL } from "./projectActivity";

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
 * The project on screen as a pull-down: a listbox of the known projects
 * (↑↓ Home End move, Enter / click switch, Esc closes) and the form that opens
 * another repository. Focus moves into the popup while it is open and back to
 * the trigger when it closes; leaving the popup closes it.
 */
export function ProjectPicker() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const view = useAppState((s) => s.project);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
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
  const current = projects.find((p) => p.id === currentId) ?? view?.info ?? null;
  const activeIndex = Math.max(0, Math.min(active, projects.length - 1));

  useEffect(() => {
    if (open) (listRef.current ?? inputRef.current)?.focus();
  }, [open]);

  useEffect(() => {
    if (open && listRef.current) revealRow(listRef.current, activeIndex);
  }, [open, activeIndex]);

  const openPopup = () => {
    setActive(Math.max(0, projects.findIndex((p) => p.id === currentId)));
    setOpen(true);
  };
  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) triggerRef.current?.focus();
  };
  const choose = (p: ProjectInfo | undefined) => {
    if (!p || blocked) return;
    if (p.id !== currentId) void store.openProject(p.path);
    close(true);
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
        title={current ? `${current.path} · ${RING_LABEL[ring ?? "stopped"]}` : undefined}
        onClick={() => (open ? close(false) : openPopup())}
        onKeyDown={onTriggerKeyDown}
      >
        {ring ? <span className={`ring ring-${ring}`} data-testid="current-ring" /> : null}
        <span className={`name ${current ? "" : "placeholder"}`}>{label}</span>
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
              {projects.map((p, i) => {
                const ring = projectRing(p, view);
                return (
                  <li
                    key={p.id}
                    id={optionId(i)}
                    role="option"
                    aria-selected={p.id === currentId}
                    aria-disabled={blocked || undefined}
                    className={`project-option ${i === activeIndex ? "active" : ""}`}
                    title={`${p.path} · ${RING_LABEL[ring]}`}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => choose(p)}
                  >
                    <span className={`ring ring-${ring}`} data-testid={`ring-${p.id}`} />
                    <span className="name">{p.name}</span>
                    {p.id === currentId ? <Icon name="check" size={14} className="check" /> : null}
                  </li>
                );
              })}
            </ul>
          )}
          <form className="open-form" onSubmit={submit}>
            <input
              ref={inputRef}
              aria-label="開くリポジトリのパス"
              placeholder="/path/to/git/repository"
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
              title="リポジトリを開く"
              disabled={blocked || !path.trim()}
            />
          </form>
        </div>
      ) : null}
    </div>
  );
}
