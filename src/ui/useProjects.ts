// The projects as the rail and the dashboard list them: the projects of every
// host, mixed (this PC's first), each told apart by its host. Without a hosts
// registry (tests, the layout harness) the store's own list is all there is.

import { useMemo } from "react";

import type { ProjectInfo } from "../api/generated";
import { type HostEntry, type HostKey, type HostProject, LOCAL_HOST } from "../store/hosts";
import type { ProjectView } from "../store/project";
import { useActiveHost, useAllProjects, useHostsRegistry, useHostsState } from "../store/useHosts";
import { useAppState, useStore } from "../store/useStore";
import type { Ring } from "./branchTree";
import { projectRing } from "./projectActivity";

/** Whether `row` is the project on screen (`view` is the active host's). */
export function isCurrentRow(row: HostProject, activeHost: HostKey, view: ProjectView | null): boolean {
  return view !== null && row.host === activeHost && row.project.id === view.info.id;
}

/**
 * The rows of the rail: the projects that are open in their core (`open`), and
 * the one on screen, which is listed even before the core says it is open (or
 * when the list does not have it yet). The order is the list's.
 */
export function openedRows(rows: readonly HostProject[], activeHost: HostKey, view: ProjectView | null): HostProject[] {
  const opened = rows.filter((row) => row.project.open || isCurrentRow(row, activeHost, view));
  if (view === null || opened.some((row) => isCurrentRow(row, activeHost, view))) return opened;
  return [...opened, { host: activeHost, remote: activeHost !== LOCAL_HOST, hostName: null, project: view.info }];
}

export interface Projects {
  /** Every project of every host. */
  rows: readonly HostProject[];
  /** The host whose store is on screen (the one `useStore` is the store of). */
  activeHost: HostKey;
  /** The entry of the active host (`undefined` without a registry, or for a host that is gone). */
  activeEntry: HostEntry | undefined;
  /** The project on screen, as the active store has it. */
  view: ProjectView | null;
  entryOf: (host: HostKey) => HostEntry | undefined;
  isCurrent: (row: HostProject) => boolean;
  /** Whether the row cannot be opened now (its host is not connected, or the active store is busy opening). */
  isBlocked: (row: HostProject) => boolean;
  /** The row's ring: from the live view for the project on screen, from the core's last word for any other. */
  ringOf: (row: HostProject) => Ring;
  /** Shows the row's project: its host becomes the active one and the project is opened there, unless it is on screen. */
  select: (row: HostProject) => void;
}

export function useProjects(): Projects {
  const store = useStore();
  const registry = useHostsRegistry();
  const own = useAppState((s) => s.projects);
  const everyones = useAllProjects();
  const activeHost = useActiveHost();
  const hostEntries = useHostsState((s) => s.hosts);
  const view = useAppState((s) => s.project);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const rows = useMemo(
    () => everyones ?? own.map((project: ProjectInfo): HostProject => ({ host: activeHost, remote: false, hostName: null, project })),
    [everyones, own, activeHost],
  );
  const entryOf = (host: HostKey) => hostEntries?.find((h) => h.key === host);
  const isCurrent = (row: HostProject) => isCurrentRow(row, activeHost, view);
  // Another host's projects are as available as that host's connection is.
  const isBlocked = (row: HostProject) => (row.host === activeHost ? !connected || opening : entryOf(row.host)?.connection !== "open");
  const ringOf = (row: HostProject) => projectRing(row.project, row.host === activeHost ? view : null);
  const select = (row: HostProject) => {
    if (isBlocked(row) || isCurrent(row)) return;
    if (registry) void registry.selectProject(row.host, row.project);
    else void store.openProject(row.project.path);
  };
  return { rows, activeHost, activeEntry: entryOf(activeHost), view, entryOf, isCurrent, isBlocked, ringOf, select };
}
