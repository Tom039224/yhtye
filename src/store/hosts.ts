// The hosts this app shows (core-design.md §9, §18.9): this app's own core and
// the other Yhtye hosts it is a client of. There is one {@link AppStore} per
// host, each on its own transport, and the screen is always drawn from the
// store of the host of the selected project — the store the {@link
// HostsRegistry} calls active. The projects of every host are read as one list
// (`projects`), mixed, each with its host.
//
// The registry:
// - makes a store for the local core, and one for each host the shell reports
//   (`HostsApi`: the list, and a notice when it or a host's state changes);
//   the store of a host stays while the host is paired with, whatever its
//   connection does, and goes when the host is forgotten;
// - tells each remote host's transport how the shell's connection to it is
//   doing (`HostLink`), and its store what the shell made of its version;
// - keeps the pairing links (`yhtye://pair?…`) that wait to be shown to the
//   person — nothing pairs by itself, the person's "connect" does.
//
// Project ids repeat across hosts (they come from directory names): what is
// kept or keyed by a project is keyed by the host as well, see {@link scopedKey}.

import type { ProjectInfo } from "../api/generated";
import type {
  HostCompatibility,
  HostNotice,
  HostsApi,
  PairingInput,
  PairingPreview,
  RemoteHostInfo,
  RemoveOutcome,
} from "../api/remoteHosts";
import { toCommandError } from "../api/transport";
import type { AppStore } from "./app";

/** Which host: `"local"` for this app's own core, else the `server_id` of a remote host. */
export type HostKey = string;

export const LOCAL_HOST: HostKey = "local";

/**
 * What goes in front of the keys of a host's projects ("" for the local core,
 * so what was saved before hosts existed is still found).
 */
export function hostScope(host: HostKey): string {
  return host === LOCAL_HOST ? "" : `host:${host}:`;
}

/** `key` (a project id, a preference key, a React key) of `host`. */
export function scopedKey(host: HostKey, key: string): string {
  return hostScope(host) + key;
}

/** What the registry needs of the transport behind a remote host's store. */
export interface HostLink {
  /** The shell's report of the host: its connection, ... */
  apply(info: RemoteHostInfo): void;
  /** The host is forgotten. */
  close(): void;
}

export interface RemoteHostStore {
  store: AppStore;
  link: HostLink;
}

/** Makes the store of a remote host; `scope` is what {@link hostScope} says. */
export type CreateRemoteStore = (info: RemoteHostInfo, scope: string) => RemoteHostStore;

/** One host, as the UI sees it. Replaced (not changed) when something in it changes. */
export interface HostEntry {
  readonly key: HostKey;
  readonly remote: boolean;
  readonly store: AppStore;
  /** What the shell knows of a remote host (`null` for the local core). */
  readonly info: RemoteHostInfo | null;
  /** The machine's name: a remote host's as the shell has it, the local core's once it has answered a ping. */
  readonly name: string | null;
  /** The state of this host's store connection. */
  readonly connection: "connecting" | "open" | "closed";
  /** The remote host's version against this app's (`null`: the local core, or not known yet). */
  readonly compatibility: HostCompatibility | null;
}

/** A project of a host, in the list of every host's projects. */
export interface HostProject {
  readonly host: HostKey;
  readonly remote: boolean;
  /** The host's machine name, if known. */
  readonly hostName: string | null;
  readonly project: ProjectInfo;
}

export interface HostsState {
  /** The host whose store draws the screen. */
  readonly active: HostKey;
  readonly activeStore: AppStore;
  /** The local core first, then the remote hosts in the order they were paired. */
  readonly hosts: readonly HostEntry[];
  /** The projects of every host, mixed: the local core's first. */
  readonly projects: readonly HostProject[];
  /** Pairing links that arrived (a click on a `yhtye://pair?…` link) and wait to be shown to the person; the first goes first. */
  readonly pairLinks: readonly string[];
  /** Remote hosts can be managed (not in a browser on the dev bridge). */
  readonly remoteAvailable: boolean;
}

export interface HostsRegistryOptions {
  /** The store of this app's own core. */
  local: AppStore;
  /** The shell's host management; `null` where there is none (the dev bridge in a browser, tests). */
  api: HostsApi | null;
  createRemote: CreateRemoteStore;
}

interface Remote {
  info: RemoteHostInfo;
  store: AppStore;
  link: HostLink;
  stopListening: () => void;
}

export class HostsRegistry {
  private readonly local: AppStore;
  private readonly api: HostsApi | null;
  private readonly createRemote: CreateRemoteStore;
  private readonly remotes = new Map<HostKey, Remote>();
  private readonly listeners = new Set<() => void>();
  private readonly stopFns: (() => void)[] = [];
  private stopLocalListening: (() => void) | null = null;
  private active: HostKey = LOCAL_HOST;
  private pairLinks: readonly string[] = [];
  private refreshing = false;
  private refreshAgain = false;
  private started = false;
  private stopped = false;
  private projectsMemo: { signature: unknown[]; value: readonly HostProject[] } | null = null;
  private state: HostsState;

  constructor(options: HostsRegistryOptions) {
    this.local = options.local;
    this.api = options.api;
    this.createRemote = options.createRemote;
    this.state = this.build(null);
  }

  getState = (): HostsState => this.state;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** The store of the host whose project is on screen. */
  get activeStore(): AppStore {
    return this.state.activeStore;
  }

  // ---- lifecycle ------------------------------------------------------------

  /**
   * Starts the local store, then listens to the shell and reads the hosts (a
   * store is made for each). A failure to talk to the shell is shown as an
   * error; the local core goes on.
   */
  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    this.stopLocalListening = this.local.subscribe(() => this.publish());
    this.local.start();
    const api = this.api;
    if (!api) return;
    try {
      this.keepUntilStopped(await api.onNotice((notice) => this.onNotice(notice)));
      this.keepUntilStopped(await api.onPairLink((uri) => this.addPairLink(uri)));
      await this.refreshHosts();
      for (const uri of await api.takePairLinks()) this.addPairLink(uri);
    } catch (e) {
      this.local.showError(`リモートホストの情報を読めません: ${toCommandError(e).message}`);
    }
  }

  stop(): void {
    this.stopped = true;
    for (const stop of this.stopFns.splice(0)) stop();
    this.stopLocalListening?.();
    this.stopLocalListening = null;
    this.local.stop();
    for (const remote of this.remotes.values()) this.dispose(remote);
    this.remotes.clear();
  }

  /** `stop` of a listener that may be registered after the registry was stopped. */
  private keepUntilStopped(stop: () => void): void {
    if (this.stopped) stop();
    else this.stopFns.push(stop);
  }

  // ---- projects ---------------------------------------------------------------

  /** Shows the host's store (and so its project). Unknown hosts are ignored. */
  activate(host: HostKey): void {
    if (host === this.active || !this.storeOf(host)) return;
    this.active = host;
    this.publish();
  }

  /**
   * Shows a project of a host: that host's store becomes the active one, and
   * opens the project there unless that store has it on screen already.
   */
  async selectProject(host: HostKey, project: ProjectInfo): Promise<void> {
    const store = this.storeOf(host);
    if (!store) return;
    this.activate(host);
    if (store.getState().project?.info.id === project.id) return;
    await store.openProject(project.path);
  }

  // ---- hosts --------------------------------------------------------------------

  /** What pairing with `input` would do, for the person to confirm. Rejects with why an input is no good. */
  async previewPairing(input: PairingInput): Promise<PairingPreview> {
    return this.needApi().previewPairing(input);
  }

  /**
   * Pairs with the host `input` names, as a device called `deviceName`, and
   * connects. Rejects with the reason (a refused code, an unreachable host, ...).
   */
  async addHost(input: PairingInput, deviceName: string): Promise<RemoteHostInfo> {
    const info = await this.needApi().add(input, deviceName);
    await this.refreshHosts();
    return info;
  }

  /** Forgets a remote host and its store. */
  async removeHost(host: HostKey): Promise<RemoveOutcome> {
    const outcome = await this.needApi().remove(host);
    await this.refreshHosts();
    return outcome;
  }

  /** Connects to a remote host now (again after it refused this device, or to skip the wait). */
  async retryHost(host: HostKey): Promise<void> {
    await this.needApi().connect(host);
  }

  // ---- pairing links ----------------------------------------------------------

  /** A `yhtye://pair?…` link arrived: it waits for the person's answer. */
  addPairLink(uri: string): void {
    if (this.pairLinks.includes(uri)) return;
    this.pairLinks = [...this.pairLinks, uri];
    this.publish();
  }

  /**
   * Takes the first waiting link off the line (`null` if none): the screen
   * shows it for the person to read and answer — the link pairs nothing by
   * itself, the person's "connect" does.
   */
  takePairLink(): string | null {
    const [first, ...rest] = this.pairLinks;
    if (first === undefined) return null;
    this.pairLinks = rest;
    this.publish();
    return first;
  }

  // ---- shell reports ------------------------------------------------------------

  private onNotice(notice: HostNotice): void {
    if (notice.type === "hosts_changed") {
      void this.refreshHosts();
      return;
    }
    // A list on its way may predate this: read it again once it is in.
    if (this.refreshing) this.refreshAgain = true;
    const remote = this.remotes.get(notice.server_id);
    if (!remote) return;
    this.update(remote, { ...remote.info, status: notice.status });
    this.publish();
  }

  /** Reads the host list from the shell and brings the stores in line with it. */
  private async refreshHosts(): Promise<void> {
    const api = this.api;
    if (!api) return;
    if (this.refreshing) {
      this.refreshAgain = true;
      return;
    }
    this.refreshing = true;
    try {
      do {
        this.refreshAgain = false;
        const list = await api.list();
        if (this.stopped) return;
        this.sync(list);
      } while (this.refreshAgain && !this.stopped);
    } catch (e) {
      this.local.showError(`リモートホストの一覧を読めません: ${toCommandError(e).message}`);
    } finally {
      this.refreshing = false;
    }
  }

  private sync(list: readonly RemoteHostInfo[]): void {
    const listed = new Set(list.map((info) => info.server_id));
    for (const [key, remote] of this.remotes) {
      if (listed.has(key)) continue;
      this.remotes.delete(key);
      this.dispose(remote);
      if (this.active === key) this.active = LOCAL_HOST;
    }
    // The map keeps the order the hosts were paired in.
    const ordered = new Map<HostKey, Remote>();
    for (const info of list) {
      const remote = this.remotes.get(info.server_id) ?? this.open(info);
      this.update(remote, info);
      ordered.set(info.server_id, remote);
    }
    this.remotes.clear();
    for (const [key, remote] of ordered) this.remotes.set(key, remote);
    this.publish();
  }

  private open(info: RemoteHostInfo): Remote {
    const { store, link } = this.createRemote(info, hostScope(info.server_id));
    const stopListening = store.subscribe(() => this.publish());
    // Not on screen until it is picked: no reading of the host's meters on connecting.
    store.setForeground(false);
    store.start();
    return { info, store, link, stopListening };
  }

  private dispose(remote: Remote): void {
    remote.stopListening();
    remote.store.stop();
    remote.link.close();
  }

  /** `remote` now has `info`: the transport and the store are told. */
  private update(remote: Remote, info: RemoteHostInfo): void {
    remote.info = info;
    remote.link.apply(info);
    remote.store.noteHostCompatibility(info.status.server?.compatibility ?? null);
  }

  // ---- state --------------------------------------------------------------------

  private needApi(): HostsApi {
    if (!this.api) throw new Error("remote hosts are not available here");
    return this.api;
  }

  private storeOf(host: HostKey): AppStore | null {
    return host === LOCAL_HOST ? this.local : (this.remotes.get(host)?.store ?? null);
  }

  private publish(): void {
    const next = this.build(this.state);
    if (next !== this.state) {
      this.state = next;
      for (const listener of [...this.listeners]) listener();
    }
    // Only the store on screen reads the host's subscription meters. (After the
    // state is in place: telling a store may make it publish in turn.)
    const { hosts, activeStore } = this.state;
    for (const entry of hosts) entry.store.setForeground(entry.store === activeStore);
  }

  /** The state now; `previous` itself if nothing in it changed. */
  private build(previous: HostsState | null): HostsState {
    const hosts = this.entries(previous?.hosts ?? []);
    const projects = this.projects(hosts);
    const activeStore = hosts.find((h) => h.key === this.active)?.store ?? this.local;
    if (
      previous &&
      previous.active === this.active &&
      previous.activeStore === activeStore &&
      previous.hosts === hosts &&
      previous.projects === projects &&
      previous.pairLinks === this.pairLinks
    ) {
      return previous;
    }
    return {
      active: this.active,
      activeStore,
      hosts,
      projects,
      pairLinks: this.pairLinks,
      remoteAvailable: this.api !== null,
    };
  }

  private entries(previous: readonly HostEntry[]): readonly HostEntry[] {
    const fresh: HostEntry[] = [entryOf(LOCAL_HOST, null, this.local)];
    for (const [key, remote] of this.remotes) fresh.push(entryOf(key, remote.info, remote.store));
    // An entry that did not change stays the object it was, and so does the list of them.
    const kept = fresh.map((entry) => {
      const before = previous.find((p) => p.key === entry.key);
      return before && sameEntry(before, entry) ? before : entry;
    });
    const unchanged = kept.length === previous.length && kept.every((entry, i) => entry === previous[i]);
    return unchanged ? previous : kept;
  }

  /** The projects of every host, mixed; the same list while no host's projects or name changed. */
  private projects(hosts: readonly HostEntry[]): readonly HostProject[] {
    const lists = hosts.map((h) => h.store.getState().projects);
    const signature = hosts.flatMap((h, i) => [h.key, h.name, lists[i]]);
    const memo = this.projectsMemo;
    if (memo && memo.signature.length === signature.length && memo.signature.every((v, i) => v === signature[i])) {
      return memo.value;
    }
    const value = hosts.flatMap((h, i) =>
      lists[i].map((project): HostProject => ({ host: h.key, remote: h.remote, hostName: h.name, project })),
    );
    this.projectsMemo = { signature, value };
    return value;
  }
}

function entryOf(key: HostKey, info: RemoteHostInfo | null, store: AppStore): HostEntry {
  const state = store.getState();
  return {
    key,
    remote: info !== null,
    store,
    info,
    name: info ? info.name : state.host.name,
    connection: state.connection.state,
    compatibility: info ? (info.status.server?.compatibility ?? null) : null,
  };
}

function sameEntry(a: HostEntry, b: HostEntry): boolean {
  return (
    a.key === b.key &&
    a.store === b.store &&
    a.info === b.info &&
    a.name === b.name &&
    a.connection === b.connection &&
    a.compatibility === b.compatibility
  );
}
