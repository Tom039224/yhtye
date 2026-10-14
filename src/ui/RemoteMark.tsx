import type { HostEntry } from "../store/hosts";
import { remoteState, remoteTitle } from "./hostLabels";
import { Icon } from "./Icon";

/**
 * What marks a project that lives on another host (T-98): the remote glyph,
 * with the host's name and state as its tooltip. Grey while that host cannot
 * be used (not connected). Nothing for the local core.
 */
export function RemoteMark({ entry, size = 12 }: { entry: HostEntry | undefined; size?: number }) {
  if (!entry?.remote) return null;
  const title = remoteTitle(entry);
  return (
    <span className={`remote-mark${remoteState(entry).down ? " down" : ""}`} role="img" aria-label={`リモート · ${title}`} title={title}>
      <Icon name="remote" size={size} />
    </span>
  );
}
