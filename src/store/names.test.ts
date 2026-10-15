import { describe, expect, it } from "vitest";

import { isGroupId, isName, isTaskId, parseYhtyeBranch } from "./names";

const LONG = "x".repeat(32);

describe("group and task ids", () => {
  it("takes names of at most 32 characters and the older numbered ids", () => {
    for (const ok of ["a", "headless-server", "v2", "a-1-b", LONG]) expect(isName(ok), ok).toBe(true);
    for (const bad of ["", "-a", "a-", "a--b", "Headless", "G-1", "a_b", "a/b", `${LONG}x`]) expect(isName(bad), bad).toBe(false);
    expect(isGroupId("G-12")).toBe(true);
    expect(isGroupId("T-12")).toBe(false);
    expect(isTaskId("T-12")).toBe(true);
    expect(isTaskId("G-12")).toBe(false);
    expect(isTaskId("T-")).toBe(false);
  });
});

describe("parseYhtyeBranch", () => {
  it("reads named group and task branches", () => {
    expect(parseYhtyeBranch("yhtye/headless-server")).toEqual({ kind: "group", group: "headless-server" });
    expect(parseYhtyeBranch("yhtye/headless-server--remote-icon")).toEqual({ kind: "task", group: "headless-server", task: "remote-icon" });
    expect(parseYhtyeBranch(`yhtye/${LONG}--${LONG}`)).toEqual({ kind: "task", group: LONG, task: LONG });
  });

  it("still reads the older numbered branches", () => {
    expect(parseYhtyeBranch("yhtye/G-1")).toEqual({ kind: "group", group: "G-1" });
    expect(parseYhtyeBranch("yhtye/G-1-T-2")).toEqual({ kind: "task", group: "G-1", task: "T-2" });
    expect(parseYhtyeBranch("yhtye/G-46-T-120")).toEqual({ kind: "task", group: "G-46", task: "T-120" });
  });

  it("reads a named task in a numbered group", () => {
    expect(parseYhtyeBranch("yhtye/G-46--remote-icon")).toEqual({ kind: "task", group: "G-46", task: "remote-icon" });
  });

  it("does not take a name ending in t-<n> for a numbered task", () => {
    expect(parseYhtyeBranch("yhtye/fix-t-3")).toEqual({ kind: "group", group: "fix-t-3" });
  });

  it("is null for other branches", () => {
    for (const other of [
      "main",
      "feature/x",
      "yhtye/",
      "yhtye/Feature",
      "yhtye/a/b",
      "yhtye/a--",
      "yhtye/--a",
      "yhtye/a--b--c",
      "yhtye/a--T-1",
      `yhtye/${LONG}x`,
      "yhtye/G-1-T-",
      "xyhtye/G-1",
    ]) {
      expect(parseYhtyeBranch(other), other).toBeNull();
    }
  });
});
