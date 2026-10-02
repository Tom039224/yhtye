import { type KeyboardEvent, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";

import { Icon, type IconName } from "./Icon";

export interface RowMenuAction {
  id: string;
  label: string;
  icon: IconName;
  /** `danger` marks a destructive action. */
  tone?: "default" | "danger";
  onSelect: () => void;
}

interface RowMenuProps {
  id: string;
  /** The menu's accessible name. */
  label: string;
  /** The button that opened it; the menu sits under it (above if there is no room). */
  anchor: RefObject<HTMLElement | null>;
  actions: RowMenuAction[];
  /** Asked to close; `refocus` says whether focus goes back to the anchor. */
  onClose: (refocus: boolean) => void;
}

/** The gap to the anchor and to the window edge, in px. */
const GAP = 2;
const EDGE = 6;

interface Position {
  top: number;
  left: number;
}

/**
 * A small pop-up menu for a row's "⋯" button. It is fixed to the window (so the
 * scrolling tree does not clip it) and closes on Esc (focus returns to the
 * button), Tab, a click outside, and scrolling or resizing. ↑ ↓ Home End move
 * between the items, Enter / Space / click choose one.
 */
export function RowMenu({ id, label, anchor, actions, onClose }: RowMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<Position | null>(null);

  useLayoutEffect(() => {
    const a = anchor.current?.getBoundingClientRect();
    const m = menuRef.current?.getBoundingClientRect();
    if (!a || !m) return;
    const below = a.bottom + GAP;
    const fitsBelow = below + m.height <= window.innerHeight - EDGE;
    const top = fitsBelow ? below : Math.max(EDGE, a.top - GAP - m.height);
    const left = Math.max(EDGE, Math.min(a.right - m.width, window.innerWidth - m.width - EDGE));
    setPosition({ top, left });
  }, [anchor]);

  useEffect(() => {
    items(menuRef.current)[0]?.focus();
  }, []);

  useEffect(() => {
    const outside = (e: globalThis.MouseEvent) => {
      const target = e.target as Node;
      if (!menuRef.current?.contains(target) && !anchor.current?.contains(target)) onClose(false);
    };
    const away = () => onClose(false);
    document.addEventListener("mousedown", outside);
    window.addEventListener("resize", away);
    window.addEventListener("scroll", away, true);
    return () => {
      document.removeEventListener("mousedown", outside);
      window.removeEventListener("resize", away);
      window.removeEventListener("scroll", away, true);
    };
  }, [anchor, onClose]);

  const onKeyDown = (e: KeyboardEvent) => {
    const list = items(menuRef.current);
    const at = list.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      e.preventDefault();
      list[(to + list.length) % list.length]?.focus();
    };
    switch (e.key) {
      case "ArrowDown":
        return move(at + 1);
      case "ArrowUp":
        return move(at - 1);
      case "Home":
        return move(0);
      case "End":
        return move(list.length - 1);
      case "Escape":
      case "Tab":
        e.preventDefault();
        return onClose(true);
    }
  };

  return (
    <div
      ref={menuRef}
      id={id}
      className="row-menu"
      role="menu"
      aria-label={label}
      style={position ?? { visibility: "hidden" }}
      onKeyDown={onKeyDown}
    >
      {actions.map((a) => (
        <button
          key={a.id}
          type="button"
          role="menuitem"
          tabIndex={-1}
          className={`row-menu-item ${a.tone === "danger" ? "danger" : ""}`}
          onClick={() => {
            onClose(false);
            a.onSelect();
          }}
        >
          <Icon name={a.icon} size={14} />
          <span>{a.label}</span>
        </button>
      ))}
    </div>
  );
}

function items(menu: HTMLElement | null): HTMLElement[] {
  return menu ? [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')] : [];
}
