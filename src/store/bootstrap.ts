// Puts the app's stores together (core-design.md §9, §11): the store of this
// app's own core on the transport of the environment, and — in the Tauri app —
// the shell's host management and a store per remote host.

import { createTransport, isTauri } from "../api/create";
import { TauriHostsApi } from "../api/remoteHosts";
import { TauriTransport } from "../api/tauri";
import type { Transport } from "../api/transport";
import { AppStore } from "./app";
import { memoryDrafts, scopedDrafts } from "./drafts";
import { HostsRegistry } from "./hosts";
import { browserPrefs, type Prefs } from "./prefs";

export interface Environment {
  /** The transport of the local core (default: the environment's: Tauri IPC, or the dev bridge). */
  transport?: Transport;
  prefs?: Prefs;
  /** Whether the shell manages remote hosts (default: inside the Tauri app). Off, there is only the local core. */
  remoteHosts?: boolean;
}

/** The registry of the hosts for this environment; call `start()` on it. */
export function createHostsRegistry(env: Environment = {}): HostsRegistry {
  const prefs = env.prefs ?? browserPrefs();
  // Drafts are kept for the whole app, apart per host (their project ids may repeat).
  const drafts = memoryDrafts();
  const local = new AppStore(env.transport ?? createTransport(), { prefs, drafts });
  const remoteHosts = env.remoteHosts ?? isTauri();
  return new HostsRegistry({
    local,
    api: remoteHosts ? new TauriHostsApi() : null,
    createRemote: (info, scope) => {
      const transport = new TauriTransport(info.server_id);
      const store = new AppStore(transport, {
        prefs,
        drafts: scopedDrafts(drafts, scope),
        keyScope: scope,
        // Connecting must not start anything on the host: the person picks the project.
        reopenLast: false,
      });
      return { store, link: { apply: (host) => transport.applyHost(host), close: () => transport.close() } };
    },
  });
}
