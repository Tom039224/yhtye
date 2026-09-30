import { describe, expect, it } from "vitest";

import type { GitOverview, SessionRecord } from "../api/generated";
import { orchestratorKey } from "../store/chats";
import { applySnapshot, newProjectView, type ProjectView } from "../store/project";
import { CHAT_GIT, CHAT_INFOS, chatSnapshot, FEAT_WT, MAIN_WT } from "../test/chats";
import { PROJECT } from "../test/fixtures";
import { buildTree, chatRing, deleteBlocker, placeOf, shortAge } from "./branchTree";

const session = (chat: string, live: boolean, turn: boolean): SessionRecord => ({
  session_key: orchestratorKey(chat),
  role: "orchestrator",
  task: null,
  acp_session_id: "a",
  status: live ? "live" : "stopped",
  turn_running: turn,
  agent: null,
  cwd: null,
});

function viewWith(sessions: SessionRecord[] = []): ProjectView {
  return { ...applySnapshot(newProjectView(PROJECT), chatSnapshot()), sessions };
}

describe("buildTree", () => {
  it("makes a node per worktree, named by its branch, with its chats newest first", () => {
    const tree = buildTree(CHAT_INFOS, CHAT_GIT);
    expect(tree.worktrees.map((w) => [w.label, w.isMain, w.chats.map((c) => c.id)])).toEqual([
      ["main", true, ["C-1"]],
      ["feat/x", false, ["C-2"]],
    ]);
    expect(tree.lost).toEqual([]);
    expect(tree.otherBranches).toEqual([]);
  });

  it("hides Yhtye's internal worktrees and branches, but lists the branches nothing has checked out", () => {
    const overview: GitOverview = {
      ...CHAT_GIT,
      branches: [...CHAT_GIT.branches, { name: "topic", sha: "t".repeat(40) }],
    };
    const tree = buildTree(CHAT_INFOS, overview);
    expect(tree.worktrees.map((w) => w.path)).toEqual([MAIN_WT, FEAT_WT]);
    expect(tree.otherBranches).toEqual(["topic"]);
  });

  it("puts the chats of a worktree git no longer lists under lost", () => {
    const gone = { id: "C-3", worktree: "/gone", title: null, created_ms: 1, last_used_ms: 1 };
    const tree = buildTree([...CHAT_INFOS, gone], CHAT_GIT);
    expect(tree.lost).toEqual([gone]);
    expect(tree.worktrees.flatMap((w) => w.chats.map((c) => c.id))).not.toContain("C-3");
  });

  it("names worktrees by their directory until git has been read", () => {
    const tree = buildTree(CHAT_INFOS, null);
    expect(tree.worktrees.map((w) => w.label)).toEqual(["feat-x", "repo"]);
    expect(placeOf(CHAT_INFOS[0], null)).toEqual({ kind: "unknown" });
  });

  it("marks a worktree whose directory is gone and keeps its chats", () => {
    const overview: GitOverview = {
      ...CHAT_GIT,
      worktrees: CHAT_GIT.worktrees.map((w) => (w.path === FEAT_WT ? { ...w, missing: true } : w)),
    };
    const node = buildTree(CHAT_INFOS, overview).worktrees.find((w) => w.path === FEAT_WT);
    expect(node).toMatchObject({ missing: true, chats: [{ id: "C-2" }] });
  });
});

describe("chatRing", () => {
  it("is stopped without a session, ready with an idle one and running during a turn", () => {
    expect(chatRing(viewWith(), "C-1")).toBe("stopped");
    expect(chatRing(viewWith([session("C-1", false, false)]), "C-1")).toBe("stopped");
    expect(chatRing(viewWith([session("C-1", true, false)]), "C-1")).toBe("ready");
    expect(chatRing(viewWith([session("C-1", true, true)]), "C-1")).toBe("running");
  });
});

describe("shortAge", () => {
  const now = 10 * 24 * 3_600_000;
  it("says now under a minute, then minutes, hours and days", () => {
    expect(shortAge(now, now - 59_000)).toBe("now");
    expect(shortAge(now, now - 3 * 60_000)).toBe("3m");
    expect(shortAge(now, now - 59 * 60_000)).toBe("59m");
    expect(shortAge(now, now - 2 * 3_600_000)).toBe("2h");
    expect(shortAge(now, now - 3 * 24 * 3_600_000)).toBe("3d");
  });

  it("says now for a time in the future", () => {
    expect(shortAge(now, now + 60_000)).toBe("now");
  });
});

describe("deleteBlocker", () => {
  it("allows deleting a chat whose groups are all finished and whose orchestrator is idle", () => {
    // C-1's only group (G-1) is done.
    expect(deleteBlocker(viewWith(), "C-1")).toBeNull();
    expect(deleteBlocker(viewWith([session("C-1", true, false)]), "C-1")).toBeNull();
  });

  it("names the unfinished group that keeps a chat", () => {
    // C-2's group (G-2) is active.
    expect(deleteBlocker(viewWith(), "C-2")).toContain("G-2");
  });

  it("refuses while the orchestrator is working", () => {
    expect(deleteBlocker(viewWith([session("C-1", true, true)]), "C-1")).toContain("実行中");
  });

  it("counts a group whose merge is blocked as unfinished", () => {
    const v = viewWith();
    const state = v.state;
    if (!state) throw new Error("no state");
    const blocked = {
      ...v,
      state: { ...state, groups: state.groups.map((g) => (g.id === "G-1" ? { ...g, status: "merge_blocked" as const } : g)) },
    };
    expect(deleteBlocker(blocked, "C-1")).toContain("G-1");
  });
});
