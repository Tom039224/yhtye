import { type CSSProperties, useCallback, useState } from "react";

import { useStore } from "../store/useStore";
import { type LayoutKey, type LayoutSizes, loadLayout, saveSize } from "./layout";

/** The stored panel sizes, and a setter that persists one (`null`: back to the default). */
export function useLayoutSizes(): { sizes: LayoutSizes; setSize: (key: LayoutKey, size: number | null) => void } {
  const { prefs } = useStore();
  const [sizes, setSizes] = useState(() => loadLayout(prefs));
  const setSize = useCallback((key: LayoutKey, size: number | null) => setSizes(saveSize(prefs, key, size)), [prefs]);
  return { sizes, setSize };
}

/** An inline style holding one size as a CSS variable; nothing while the size is unset. */
export function sizeVar(name: string, size: number | undefined): CSSProperties {
  return size === undefined ? {} : ({ [name]: `${size}px` } as CSSProperties);
}

/** Sets a size variable straight on the element, so a drag repaints without re-rendering the panels. */
export function setSizeVar(element: HTMLElement | null, name: string, size: number): void {
  element?.style.setProperty(name, `${size}px`);
}
