import { createContext, type ReactNode, useContext, useSyncExternalStore } from "react";

import { type HostEntry, type HostKey, type HostProject, type HostsRegistry, LOCAL_HOST } from "./hosts";
import { StoreContext } from "./useStore";

/** The registry of the hosts; absent where only one store is given (tests, the layout harness). */
export const HostsContext = createContext<HostsRegistry | null>(null);

/**
 * Provides the registry, and the store of the host whose project is on screen
 * as the app's one store (`useStore`): the screen is always drawn from it.
 */
export function HostsProvider({ registry, children }: { registry: HostsRegistry; children: ReactNode }) {
  const store = useSyncExternalStore(registry.subscribe, () => registry.getState().activeStore);
  return (
    <HostsContext.Provider value={registry}>
      <StoreContext.Provider value={store}>{children}</StoreContext.Provider>
    </HostsContext.Provider>
  );
}

/** The registry, or `null` where there is none. */
export function useHostsRegistry(): HostsRegistry | null {
  return useContext(HostsContext);
}

const NOTHING = (): (() => void) => () => {};

/** A slice of the hosts' state (return stable references); `null` without a registry. */
export function useHostsState<T>(select: (s: ReturnType<HostsRegistry["getState"]>) => T): T | null {
  const registry = useHostsRegistry();
  return useSyncExternalStore(
    registry ? registry.subscribe : NOTHING,
    () => (registry ? select(registry.getState()) : null),
  );
}

/** The projects of every host, mixed; `null` where there is no registry (the store's own list is all there is). */
export function useAllProjects(): readonly HostProject[] | null {
  return useHostsState((s) => s.projects);
}

/** The host whose store is on screen. */
export function useActiveHost(): HostKey {
  return useHostsState((s) => s.active) ?? LOCAL_HOST;
}

/** The entry of the host whose store is on screen (`null` without a registry). */
export function useActiveHostEntry(): HostEntry | null {
  return useHostsState((s) => s.hosts.find((h) => h.key === s.active) ?? null);
}
