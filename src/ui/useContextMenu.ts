import { type MouseEvent, type RefObject, useCallback, useRef, useState } from "react";

export interface ContextMenu {
  isOpen: boolean;
  /** The element the menu opened on (it sits under it; pass it to `RowMenu`). */
  anchor: RefObject<HTMLElement | null>;
  /** The `onContextMenu` of the row: right click, the menu key and Shift + F10 open the menu. */
  onContextMenu: (e: MouseEvent<HTMLElement>) => void;
  close: () => void;
}

/** The open / closed state of a row's right-click menu (`RowMenu`), anchored to the row. */
export function useContextMenu(): ContextMenu {
  const [isOpen, setOpen] = useState(false);
  const anchor = useRef<HTMLElement | null>(null);
  const onContextMenu = useCallback((e: MouseEvent<HTMLElement>) => {
    e.preventDefault();
    e.stopPropagation();
    anchor.current = e.currentTarget;
    setOpen(true);
  }, []);
  const close = useCallback(() => setOpen(false), []);
  return { isOpen, anchor, onContextMenu, close };
}
