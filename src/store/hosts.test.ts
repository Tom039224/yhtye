// The registry of hosts (store/hosts.ts): a store per host, the active one, the
// projects of every host as one list, and what the shell reports.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { HostCompatibility } from "../api/remoteHosts";
import { CommandError } from "../api/transport";
import {
  CONNECTED,
  COMPATIBLE,
  FAR_ONLY,
  FAR_REPO,
  FakeHostsApi,
  hostInfo as info,
  newTestHosts,
  type TestHosts,
} from "../test/remoteHosts";
import { PROJECT } from "../test/fixtures";
import { chatPrefKey, LAST_PROJECT_KEY } from "./app";
import { type HostsRegistry, hostScope, LOCAL_HOST, scopedKey } from "./hosts";
import type { Prefs } from "./prefs";

let t: TestHosts;
let prefs: Prefs;
let drafts: TestHosts["drafts"];
let localTransport: TestHosts["localTransport"];
let api: FakeHostsApi;
let fars: TestHosts["fars"];
let registry: HostsRegistry;

const flush = () => vi.waitFor(() => undefined);
const hostKeys = () => registry.getState().hosts.map((h) => h.key);
const projectLabels = () => registry.getState().projects.map((p) => `${p.host}:${p.project.id}`);

beforeEach(() => {
  t = newTestHosts();
  ({ prefs, drafts, localTransport, fars, registry } = t);
  api = t.api as FakeHostsApi;
});

afterEach(() => {
  registry.stop();
});

describe("keys", () => {
  it("leaves the local core's keys as they were and puts the host in front of a remote host's", () => {
    expect(hostScope(LOCAL_HOST)).toBe("");
    expect(hostScope("sid-1")).toBe("host:sid-1:");
    expect(scopedKey(LOCAL_HOST, "repo")).toBe("repo");
    expect(scopedKey("sid-1", "repo")).toBe("host:sid-1:repo");
    expect(scopedKey("sid-1", "repo")).not.toBe(scopedKey("sid-2", "repo"));
  });
});

describe("start", () => {
  it("has only the local core before and without a shell", async () => {
    expect(hostKeys()).toEqual([LOCAL_HOST]);
    expect(registry.getState()).toMatchObject({ active: LOCAL_HOST, remoteAvailable: true, pairLinks: [] });
    expect(registry.activeStore).toBe(registry.getState().hosts[0].store);

    const alone = newTestHosts({ api: null }).registry;
    await alone.start();
    expect(alone.getState().remoteAvailable).toBe(false);
    await vi.waitFor(() => expect(alone.getState().projects.map((p) => p.project.id)).toEqual(["repo", "beta"]));
    await expect(alone.previewPairing({ kind: "uri", uri: "yhtye://pair?v=1" })).rejects.toThrow(/not available/);
    alone.stop();
  });

  it("listens before it reads the hosts, so that no notice or link is missed", async () => {
    api.hosts = [info("sid-1")];
    api.waiting = ["yhtye://pair?v=1&ip=100.64.0.9"];
    await registry.start();
    expect(api.calls).toEqual(["onNotice", "onPairLink", "list", "takePairLinks"]);
    expect(registry.getState().pairLinks).toEqual(["yhtye://pair?v=1&ip=100.64.0.9"]);
  });

  it("makes a store for each host, in the order they were paired, on its own key scope", async () => {
    api.hosts = [info("sid-2"), info("sid-1")];
    await registry.start();
    expect(hostKeys()).toEqual([LOCAL_HOST, "sid-2", "sid-1"]);
    const [local, second] = registry.getState().hosts;
    expect(local).toMatchObject({ remote: false, info: null, compatibility: null });
    expect(second).toMatchObject({ remote: true, name: "host-sid-2", compatibility: COMPATIBLE });
    expect(fars.get("sid-2")?.store.keyScope).toBe("host:sid-2:");
    expect(registry.getState().activeStore).toBe(local.store);
  });

  it("starting twice changes nothing", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await registry.start();
    expect(api.calls.filter((c) => c === "list")).toHaveLength(1);
  });

  it("shows an error, and goes on with the local core, if the shell cannot be read", async () => {
    api.failList = new Error("ipc is down");
    await registry.start();
    expect(hostKeys()).toEqual([LOCAL_HOST]);
    expect(registry.activeStore.getState().errors.map((e) => e.message).join()).toContain("ipc is down");
    // The notices of the shell are still heard.
    api.failList = null;
    api.hosts = [info("sid-1")];
    api.notice({ type: "hosts_changed" });
    await vi.waitFor(() => expect(hostKeys()).toEqual([LOCAL_HOST, "sid-1"]));
  });
});

describe("the projects of every host", () => {
  it("are one list, the local core's first, even when ids repeat", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(projectLabels()).toEqual(["local:repo", "local:beta", "sid-1:repo", "sid-1:server-only"]));
    const rows = registry.getState().projects;
    expect(rows[0]).toMatchObject({ remote: false, project: { path: "/tmp/repo" } });
    expect(rows[2]).toMatchObject({ remote: true, hostName: "host-sid-1", project: { path: "/srv/repo" } });
  });

  it("keep their list while nothing about them changes", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(registry.getState().projects).toHaveLength(4));
    const before = registry.getState();
    // A change of a store that touches no project (an error banner) republishes nothing the list depends on.
    registry.activeStore.showError("something");
    const after = registry.getState();
    expect(after.projects).toBe(before.projects);
    expect(after.hosts[1]).toBe(before.hosts[1]);
  });

  it("follow the projects a host reports changed", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(projectLabels()).toContain("sid-1:server-only"));
    const far = fars.get("sid-1");
    if (!far) throw new Error("no host");
    far.core.others = [];
    await far.store.refreshProjects();
    expect(projectLabels()).toEqual(["local:repo", "local:beta", "sid-1:repo"]);
  });
});

describe("the active host", () => {
  beforeEach(async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(projectLabels()).toContain("sid-1:server-only"));
  });

  it("changes to the host of the project that is picked, which is opened there and nowhere else", async () => {
    const row = registry.getState().projects.find((p) => p.host === "sid-1" && p.project.id === "server-only");
    if (!row) throw new Error("no row");
    await registry.selectProject(row.host, row.project);
    expect(registry.getState().active).toBe("sid-1");
    expect(registry.activeStore).toBe(fars.get("sid-1")?.store);
    expect(fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toEqual(["/srv/server-only"]);
    expect(localTransport.callsOf("open_project")).toEqual([]);
    expect(registry.activeStore.getState().project?.info.id).toBe("server-only");
    // The other host's screen is untouched.
    expect(registry.getState().hosts[0].store.getState().project).toBeNull();
  });

  it("is not reopened when the host has that project on screen already, and returns with the local project picked", async () => {
    await registry.selectProject(LOCAL_HOST, PROJECT);
    expect(localTransport.callsOf("open_project")).toHaveLength(1);
    await registry.selectProject("sid-1", FAR_ONLY);
    await registry.selectProject(LOCAL_HOST, PROJECT);
    expect(registry.getState().active).toBe(LOCAL_HOST);
    expect(localTransport.callsOf("open_project")).toHaveLength(1);
    await registry.selectProject("sid-1", FAR_ONLY);
    expect(fars.get("sid-1")?.transport.callsOf("open_project")).toHaveLength(1);
  });

  it("ignores a host that is not there", async () => {
    await registry.selectProject("sid-9", FAR_ONLY);
    registry.activate("sid-9");
    expect(registry.getState().active).toBe(LOCAL_HOST);
  });

  it("tells those who listen when it changes", async () => {
    const seen: string[] = [];
    const stop = registry.subscribe(() => seen.push(registry.getState().active));
    registry.activate("sid-1");
    registry.activate("sid-1");
    registry.activate(LOCAL_HOST);
    stop();
    registry.activate("sid-1");
    expect(seen).toEqual(["sid-1", LOCAL_HOST]);
  });
});

describe("the subscription meters", () => {
  const usageCalls = (id: string | null) =>
    (id === null ? localTransport : fars.get(id)?.transport)?.callsOf("get_usage").length ?? -1;

  it("are read only for the host on screen: a host that is only connected to is not asked", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(usageCalls(null)).toBeGreaterThan(0));
    await vi.waitFor(() => expect(projectLabels()).toContain("sid-1:server-only"));
    expect(usageCalls("sid-1")).toBe(0);

    registry.activate("sid-1");
    await vi.waitFor(() => expect(usageCalls("sid-1")).toBe(1));
    // And back: the local core's are read again at once.
    const local = usageCalls(null);
    registry.activate(LOCAL_HOST);
    await vi.waitFor(() => expect(usageCalls(null)).toBe(local + 1));
    expect(usageCalls("sid-1")).toBe(1);
  });
});

describe("what a remote host's store saves", () => {
  it("is kept apart from the local core's by the host's key scope", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    await registry.selectProject(LOCAL_HOST, PROJECT);
    await registry.selectProject("sid-1", FAR_REPO);
    await vi.waitFor(() => expect(registry.activeStore.getState().project?.phase).toBe("ready"));
    expect(prefs.get(LAST_PROJECT_KEY)).toBe("/tmp/repo");
    expect(prefs.get("host:sid-1:" + LAST_PROJECT_KEY)).toBe("/srv/repo");
    // The chat shown is remembered per host, too: the same project id on two hosts.
    const far = registry.activeStore;
    far.selectChat("C-1");
    expect(prefs.get(chatPrefKey("repo", "host:sid-1:"))).toBe("C-1");
    expect(prefs.get(chatPrefKey("repo"))).toBeNull();
    // So are the drafts.
    far.drafts.set("repo", "C-1", "typed on the server");
    expect(registry.getState().hosts[0].store.drafts.get("repo", "C-1")).toBe("");
    expect(drafts.get("host:sid-1:repo", "C-1")).toBe("typed on the server");
  });

  it("does not open the project that was open last when connecting to a host", async () => {
    prefs.set("host:sid-1:" + LAST_PROJECT_KEY, "/srv/repo");
    api.hosts = [info("sid-1")];
    await registry.start();
    await vi.waitFor(() => expect(projectLabels()).toContain("sid-1:server-only"));
    expect(fars.get("sid-1")?.transport.callsOf("open_project")).toEqual([]);
    expect(fars.get("sid-1")?.store.getState().project).toBeNull();
  });
});

describe("what the shell reports", () => {
  beforeEach(async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
  });

  it("is a host's state changing: its transport and its store are told, and the entry is replaced", async () => {
    const far = fars.get("sid-1");
    const before = registry.getState();
    const incompatible: HostCompatibility = { type: "incompatible", host: "0.2.0", app: "0.1.0" };
    api.notice({ type: "status", server_id: "sid-1", status: { ...info("sid-1").status, server: { host: "host-sid-1", version: "0.2.0", headless: false, compatibility: incompatible } } });
    const entry = registry.getState().hosts[1];
    expect(entry.compatibility).toEqual(incompatible);
    expect(entry).not.toBe(before.hosts[1]);
    expect(registry.getState().hosts[0]).toBe(before.hosts[0]);
    expect(far?.store.getState().host.compatibility).toEqual(incompatible);
    expect(far?.applied.at(-1)?.status.server?.version).toBe("0.2.0");

    api.notice({ type: "status", server_id: "sid-1", status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 1000, reason: "refused" }, server: null } });
    expect(registry.getState().hosts[1]).toMatchObject({ connection: "closed", compatibility: null });
    expect(far?.store.getState().connection).toMatchObject({ state: "closed", retryInMs: 1000 });
    api.notice({ type: "status", server_id: "sid-1", status: { connection: CONNECTED, server: null } });
    await vi.waitFor(() => expect(far?.store.getState().connection.state).toBe("open"));
  });

  it("is ignored for a host it does not know", () => {
    const before = registry.getState();
    api.notice({ type: "status", server_id: "sid-9", status: { connection: CONNECTED, server: null } });
    expect(registry.getState()).toBe(before);
  });

  it("is a host added: the list is read again and the host has a store", async () => {
    api.hosts.push(info("sid-2"));
    api.notice({ type: "hosts_changed" });
    await vi.waitFor(() => expect(hostKeys()).toEqual([LOCAL_HOST, "sid-1", "sid-2"]));
    expect(fars.get("sid-2")?.applied).toHaveLength(1);
    // The store that was there is the same one.
    expect(fars.size).toBe(2);
  });

  it("is a host renamed: the entry has the new name", async () => {
    api.hosts = [{ ...info("sid-1"), name: "renamed" }];
    api.notice({ type: "hosts_changed" });
    await vi.waitFor(() => expect(registry.getState().hosts[1].name).toBe("renamed"));
    expect(fars.size).toBe(1);
  });

  it("is a host removed: its store goes, and the screen returns to the local core if it was on it", async () => {
    await registry.selectProject("sid-1", FAR_ONLY);
    expect(registry.getState().active).toBe("sid-1");
    const far = fars.get("sid-1");
    api.hosts = [];
    api.notice({ type: "hosts_changed" });
    await vi.waitFor(() => expect(hostKeys()).toEqual([LOCAL_HOST]));
    expect(far?.closed).toBe(true);
    expect(registry.getState().active).toBe(LOCAL_HOST);
    expect(registry.activeStore).toBe(registry.getState().hosts[0].store);
    expect(projectLabels().every((p) => p.startsWith("local:"))).toBe(true);
    // Nothing of the forgotten store reaches the registry any more.
    const before = registry.getState();
    far?.store.showError("late");
    expect(registry.getState()).toBe(before);
  });

  it("is read again when a state came while the list was on its way", async () => {
    let release = () => {};
    api.listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    api.calls.length = 0;
    api.notice({ type: "hosts_changed" });
    await flush();
    // The list answered before this state; a second read follows.
    api.notice({ type: "status", server_id: "sid-1", status: info("sid-1", { state: "revoked" }).status });
    api.hosts = [info("sid-1", { state: "revoked" })];
    api.listGate = null;
    release();
    await vi.waitFor(() => expect(api.calls.filter((c) => c === "list")).toHaveLength(2));
    await vi.waitFor(() => expect(registry.getState().hosts[1].info?.status.connection).toEqual({ state: "revoked" }));
  });
});

describe("pairing", () => {
  beforeEach(async () => {
    await registry.start();
  });

  it("waits for the person: a link is queued, once, and answered one at a time", () => {
    const a = "yhtye://pair?v=1&ip=100.64.0.1";
    const b = "yhtye://pair?v=1&ip=100.64.0.2";
    api.link(a);
    api.link(a);
    api.link(b);
    expect(registry.getState().pairLinks).toEqual([a, b]);
    expect(api.calls).not.toContain("add uri");
    registry.dismissPairLink();
    expect(registry.getState().pairLinks).toEqual([b]);
    registry.dismissPairLink();
    registry.dismissPairLink();
    expect(registry.getState().pairLinks).toEqual([]);
  });

  it("previews what pairing would do without pairing", async () => {
    expect(await registry.previewPairing({ kind: "uri", uri: "yhtye://pair?v=1" })).toBe(api.preview);
    expect(api.calls.some((c) => c.startsWith("add"))).toBe(false);
  });

  it("adds the host under the name given and gives it a store", async () => {
    const added = await registry.addHost({ kind: "uri", uri: "yhtye://pair?v=1" }, "my laptop");
    expect(added.server_id).toBe("sid-new");
    expect(api.calls).toContain("add uri my laptop");
    expect(hostKeys()).toEqual([LOCAL_HOST, "sid-new"]);
  });

  it("rejects with why pairing failed, and adds nothing", async () => {
    api.addError = new CommandError("forbidden", "the pairing code was refused");
    await expect(registry.addHost({ kind: "uri", uri: "yhtye://pair?v=1" }, "pc")).rejects.toMatchObject({ code: "forbidden" });
    expect(hostKeys()).toEqual([LOCAL_HOST]);
  });

  it("forgets a host and connects to one again", async () => {
    await registry.addHost({ kind: "uri", uri: "yhtye://pair?v=1" }, "pc");
    await registry.retryHost("sid-new");
    expect(api.calls).toContain("connect sid-new");
    await registry.removeHost("sid-new");
    expect(hostKeys()).toEqual([LOCAL_HOST]);
  });
});

describe("stop", () => {
  it("stops listening to the shell, and closes the stores and links", async () => {
    api.hosts = [info("sid-1")];
    await registry.start();
    registry.stop();
    expect(api.stoppedNotices).toBe(1);
    expect(api.stoppedLinks).toBe(1);
    expect(fars.get("sid-1")?.closed).toBe(true);
    // A notice that comes late changes nothing.
    const before = registry.getState();
    api.notice({ type: "hosts_changed" });
    await flush();
    expect(registry.getState()).toBe(before);
  });
});
