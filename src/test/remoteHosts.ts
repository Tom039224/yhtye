// Test-only: the shell's host management and the remote hosts it connects to,
// as fakes, around a real `HostsRegistry` (stores on memory transports).

import type { ProjectInfo } from "../api/generated";
import {
  type HostConnection,
  type HostCompatibility,
  type HostNotice,
  type HostsApi,
  type HostToken,
  linkStatus,
  type PairingInput,
  type PairingPreview,
  type RemoteHostInfo,
  type RemoveOutcome,
} from "../api/remoteHosts";
import { AppStore } from "../store/app";
import { memoryDrafts, scopedDrafts } from "../store/drafts";
import { HostsRegistry } from "../store/hosts";
import { memoryPrefs, type Prefs } from "../store/prefs";
import { CHAT_LOG, chatSnapshot } from "./chats";
import { FULL_RUN, PROJECT } from "./fixtures";
import { FakeCore, MemoryTransport } from "./memoryTransport";

export const BETA: ProjectInfo = { id: "beta", name: "beta", path: "/tmp/beta", open: true, running: false };
/** The remote host has a project whose id is the local one's (ids come from directory names). */
export const FAR_REPO: ProjectInfo = { id: "repo", name: "repo", path: "/srv/repo", open: false, running: false };
export const FAR_ONLY: ProjectInfo = { id: "server-only", name: "server-only", path: "/srv/server-only", open: false, running: false };

export const CONNECTED: HostConnection = { state: "connected", session: 1 };
export const COMPATIBLE: HostCompatibility = { type: "compatible" };

export function hostInfo(id: string, connection: HostConnection = CONNECTED, compatibility: HostCompatibility = COMPATIBLE): RemoteHostInfo {
  return {
    server_id: id,
    name: `host-${id}`,
    hosts: ["100.64.0.7"],
    port: 1423,
    device_id: "dev",
    device_name: "this pc",
    added_ms: 1,
    token_in_keyring: true,
    token: { place: "keyring" },
    status: { connection, server: { host: `host-${id}`, version: "0.1.0", headless: false, compatibility } },
  };
}

/** What the shell offers. Everything it is asked is recorded, and the notices are pushed by the test. */
export class FakeHostsApi implements HostsApi {
  hosts: RemoteHostInfo[] = [];
  /** Links that came before the webview listened. */
  waiting: string[] = [];
  calls: string[] = [];
  /** When set, `list` answers with it instead (to answer late). */
  listGate: Promise<void> | null = null;
  failList: Error | null = null;
  private noticeHandler: ((n: HostNotice) => void) | null = null;
  private linkHandler: ((uri: string) => void) | null = null;
  stoppedNotices = 0;
  stoppedLinks = 0;
  preview: PairingPreview = { hosts: ["100.64.0.7"], port: 1423, server_id: "sid-new", default_device_name: "this pc" };
  addError: Error | null = null;
  /** What the shell says of the token it kept for a host just added. */
  addedToken: HostToken = { place: "keyring" };
  /** What was asked of `add`, in order. */
  inputs: PairingInput[] = [];

  async list(): Promise<RemoteHostInfo[]> {
    this.calls.push("list");
    const answer = structuredClone(this.hosts);
    if (this.failList) throw this.failList;
    if (this.listGate) await this.listGate;
    return answer;
  }
  async previewPairing(input: PairingInput): Promise<PairingPreview> {
    this.calls.push(`preview ${input.kind}`);
    return this.preview;
  }
  async add(input: PairingInput, deviceName: string): Promise<RemoteHostInfo> {
    this.calls.push(`add ${input.kind} ${deviceName}`);
    this.inputs.push(input);
    if (this.addError) throw this.addError;
    const added = { ...hostInfo("sid-new", { state: "connecting", attempt: 0 }), token: this.addedToken, device_name: deviceName };
    this.hosts.push(added);
    return added;
  }
  async remove(serverId: string): Promise<RemoveOutcome> {
    this.calls.push(`remove ${serverId}`);
    this.hosts = this.hosts.filter((h) => h.server_id !== serverId);
    return { token_left_in_keyring: null };
  }
  async connect(serverId: string): Promise<void> {
    this.calls.push(`connect ${serverId}`);
  }
  async onNotice(handler: (n: HostNotice) => void): Promise<() => void> {
    this.calls.push("onNotice");
    this.noticeHandler = handler;
    return () => {
      this.stoppedNotices++;
    };
  }
  async onPairLink(handler: (uri: string) => void): Promise<() => void> {
    this.calls.push("onPairLink");
    this.linkHandler = handler;
    return () => {
      this.stoppedLinks++;
    };
  }
  async takePairLinks(): Promise<string[]> {
    this.calls.push("takePairLinks");
    return this.waiting;
  }
  notice(n: HostNotice): void {
    this.noticeHandler?.(n);
  }
  link(uri: string): void {
    this.linkHandler?.(uri);
  }
}

/** A remote host as the registry makes it: a store on a memory transport, and what the registry told it. */
export class Far {
  readonly transport = new MemoryTransport({ state: "connecting" });
  readonly core: FakeCore;
  readonly applied: RemoteHostInfo[] = [];
  closed = false;
  store!: AppStore;
  constructor(readonly id: string) {
    this.core = new FakeCore(FAR_REPO, chatSnapshot());
    this.core.log = CHAT_LOG;
    this.core.others = [FAR_ONLY];
    this.core.host = `host-${id}`;
    this.core.attach(this.transport);
  }
}

export interface TestHosts {
  registry: HostsRegistry;
  api: FakeHostsApi | null;
  /** The remote hosts the registry made a store for. */
  fars: Map<string, Far>;
  localTransport: MemoryTransport;
  localCore: FakeCore;
  prefs: Prefs;
  drafts: ReturnType<typeof memoryDrafts>;
}

/**
 * A registry whose local core knows `repo` and `beta`, and whose remote hosts know `repo` (another one) and `server-only`.
 * `configureFar` shapes a remote host's core (what it answers, its devices) before its store first asks.
 */
export function newTestHosts(options: { api?: FakeHostsApi | null; configureFar?: (far: Far) => void } = {}): TestHosts {
  const prefs = memoryPrefs();
  const drafts = memoryDrafts();
  const localTransport = new MemoryTransport();
  const localCore = new FakeCore(PROJECT, FULL_RUN.start);
  localCore.others = [BETA];
  localCore.attach(localTransport);
  const api = options.api === undefined ? new FakeHostsApi() : options.api;
  const fars = new Map<string, Far>();
  const local = new AppStore(localTransport, { prefs, drafts });
  const registry = new HostsRegistry({
    local,
    api,
    createRemote: (host, scope) => {
      const far = new Far(host.server_id);
      options.configureFar?.(far);
      far.store = new AppStore(far.transport, {
        prefs,
        drafts: scopedDrafts(drafts, scope),
        keyScope: scope,
        reopenLast: false,
      });
      fars.set(host.server_id, far);
      return {
        store: far.store,
        link: {
          apply: (h) => {
            far.applied.push(h);
            far.transport.setStatus(linkStatus(h.status.connection));
          },
          close: () => {
            far.closed = true;
          },
        },
      };
    },
  });
  return { registry, api, fars, localTransport, localCore, prefs, drafts };
}
