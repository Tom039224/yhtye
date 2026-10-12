import { useCallback, useEffect, useRef, useState } from "react";

import type { SettingsKind } from "../api/generated";
import type { SettingsChange } from "../store/app";
import { useStore } from "../store/useStore";

interface Options {
  /** Reads the section's settings again. */
  reload: () => void;
  /** A save of this section is in flight: a change is looked at once it is done. */
  busy: boolean;
  /** The user is in the middle of an edit a reload would throw away. */
  isEditing: () => boolean;
  /** The change is what the section shows already (e.g. its own save). */
  isShown?: (change: SettingsChange) => boolean;
}

/**
 * Keeps a settings section up to date when settings of `kinds` are changed by
 * any client (T-65): it is read again, unless the user is editing it — then
 * `stale` is set for a banner that offers `reloadNow`. A change that arrives
 * while the section saves waits for the save to finish (it may be the save's
 * own announcement). A missed change (reconnect, resync) counts as any kind.
 */
export function useSettingsChanges(kinds: SettingsKind[], options: Options): { stale: boolean; reloadNow: () => void } {
  const store = useStore();
  const [stale, setStale] = useState(false);
  const opts = useRef(options);
  opts.current = options;
  /** Every change that came during a save (the save's own notice must not hide another client's). */
  const pending = useRef<SettingsChange[]>([]);
  const kindKey = kinds.join(",");

  const consider = useCallback((change: SettingsChange) => {
    const o = opts.current;
    if (o.isShown?.(change)) return;
    if (o.busy) {
      pending.current.push(change);
    } else if (o.isEditing()) {
      setStale(true);
    } else {
      setStale(false); // read again now: the banner would be wrong
      o.reload();
    }
  }, []);

  useEffect(
    () =>
      store.onSettingsChanged((change) => {
        if (change.kind === null || kindKey.split(",").includes(change.kind)) consider(change);
      }),
    [store, kindKey, consider],
  );

  useEffect(() => {
    if (options.busy || pending.current.length === 0) return;
    const queued = pending.current;
    pending.current = [];
    // One reload covers them all; only changes not on screen yet need it.
    const next = queued.find((c) => !opts.current.isShown?.(c));
    if (next) consider(next);
  }, [options.busy, consider]);

  const reloadNow = useCallback(() => {
    setStale(false);
    opts.current.reload();
  }, []);
  return { stale, reloadNow };
}

/** Whether keyboard focus is in a text field inside `root` (an edit in progress). */
export function typingIn(root: HTMLElement | null): boolean {
  const el = document.activeElement;
  if (!root || !el || !root.contains(el)) return false;
  return el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && el.type !== "checkbox" && el.type !== "radio");
}
