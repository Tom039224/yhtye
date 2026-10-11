import { useCallback, useEffect, useState } from "react";

/**
 * The tabs of the bottom panel: git is always there, and every task whose agent
 * output was opened gets a tab next to it. `active` is the task shown, `null`
 * while the panel shows git.
 */
export interface BottomTabs {
  /** The open task tabs, in the order they were opened. */
  readonly tasks: readonly string[];
  readonly active: string | null;
}

export const NO_TABS: BottomTabs = { tasks: [], active: null };

/** Opens the task's tab (once) and shows it; a task that is already open just becomes the active tab. */
export function openTask(tabs: BottomTabs, task: string): BottomTabs {
  if (tabs.active === task) return tabs;
  return { tasks: tabs.tasks.includes(task) ? tabs.tasks : [...tabs.tasks, task], active: task };
}

/** Shows git (`null`) or an open task tab; a task that is not open is ignored. */
export function showTab(tabs: BottomTabs, active: string | null): BottomTabs {
  if (tabs.active === active || (active !== null && !tabs.tasks.includes(active))) return tabs;
  return { ...tabs, active };
}

/**
 * Closes the task's tab. Closing the shown tab shows its neighbour (the tab that
 * took its place, else the one before), and git once no task tab is left.
 */
export function closeTask(tabs: BottomTabs, task: string): BottomTabs {
  const index = tabs.tasks.indexOf(task);
  if (index < 0) return tabs;
  const rest = tabs.tasks.filter((t) => t !== task);
  if (tabs.active !== task) return { ...tabs, tasks: rest };
  return { tasks: rest, active: rest[index] ?? rest[index - 1] ?? null };
}

export interface BottomTabsControl {
  tabs: BottomTabs;
  open: (task: string) => void;
  show: (active: string | null) => void;
  close: (task: string) => void;
}

/** The panel's tabs, which start over whenever `resetKey` (the chat) changes: its tasks belong to the previous chat's column. */
export function useBottomTabs(resetKey: string): BottomTabsControl {
  const [tabs, setTabs] = useState<BottomTabs>(NO_TABS);
  useEffect(() => setTabs(NO_TABS), [resetKey]);
  const open = useCallback((task: string) => setTabs((t) => openTask(t, task)), []);
  const show = useCallback((active: string | null) => setTabs((t) => showTab(t, active)), []);
  const close = useCallback((task: string) => setTabs((t) => closeTask(t, task)), []);
  return { tabs, open, show, close };
}
