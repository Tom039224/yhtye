import { createContext, useContext, useSyncExternalStore } from "react";

import type { AppState, AppStore } from "./app";

export const StoreContext = createContext<AppStore | null>(null);

export function useStore(): AppStore {
  const store = useContext(StoreContext);
  if (!store) throw new Error("StoreContext is not provided");
  return store;
}

/** Subscribes to a slice of the app state (return stable references). */
export function useAppState<T>(select: (s: AppState) => T): T {
  const store = useStore();
  return useSyncExternalStore(store.subscribe, () => select(store.getState()));
}
