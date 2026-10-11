import { describe, expect, it } from "vitest";

import { type BottomTabs, closeTask, NO_TABS, openTask, showTab } from "./bottomTabs";

const tabs = (tasks: string[], active: string | null): BottomTabs => ({ tasks, active });

describe("openTask", () => {
  it("opens a task's tab next to the others and shows it", () => {
    expect(openTask(NO_TABS, "T-1")).toEqual(tabs(["T-1"], "T-1"));
    expect(openTask(tabs(["T-1"], "T-1"), "T-2")).toEqual(tabs(["T-1", "T-2"], "T-2"));
  });

  it("shows a task that is already open instead of opening it twice", () => {
    expect(openTask(tabs(["T-1", "T-2"], "T-2"), "T-1")).toEqual(tabs(["T-1", "T-2"], "T-1"));
    expect(openTask(tabs(["T-1", "T-2"], null), "T-2")).toEqual(tabs(["T-1", "T-2"], "T-2"));
  });

  it("keeps the same state when the task is already the shown one", () => {
    const shown = tabs(["T-1"], "T-1");
    expect(openTask(shown, "T-1")).toBe(shown);
  });
});

describe("showTab", () => {
  it("switches between git and the open tasks", () => {
    const open = tabs(["T-1", "T-2"], "T-2");
    expect(showTab(open, null)).toEqual(tabs(["T-1", "T-2"], null));
    expect(showTab(open, "T-1")).toEqual(tabs(["T-1", "T-2"], "T-1"));
  });

  it("ignores a task that is not open and keeps the same state when nothing changes", () => {
    const open = tabs(["T-1"], "T-1");
    expect(showTab(open, "T-9")).toBe(open);
    expect(showTab(open, "T-1")).toBe(open);
    expect(showTab(NO_TABS, null)).toBe(NO_TABS);
  });
});

describe("closeTask", () => {
  it("closes a tab that is not shown without changing what is shown", () => {
    expect(closeTask(tabs(["T-1", "T-2", "T-3"], "T-2"), "T-3")).toEqual(tabs(["T-1", "T-2"], "T-2"));
    expect(closeTask(tabs(["T-1", "T-2"], null), "T-1")).toEqual(tabs(["T-2"], null));
  });

  it("shows the tab that took the closed one's place, else the one before", () => {
    expect(closeTask(tabs(["T-1", "T-2", "T-3"], "T-2"), "T-2")).toEqual(tabs(["T-1", "T-3"], "T-3"));
    expect(closeTask(tabs(["T-1", "T-2", "T-3"], "T-3"), "T-3")).toEqual(tabs(["T-1", "T-2"], "T-2"));
    expect(closeTask(tabs(["T-1", "T-2"], "T-1"), "T-1")).toEqual(tabs(["T-2"], "T-2"));
  });

  it("goes back to git when the last task tab is closed", () => {
    expect(closeTask(tabs(["T-1"], "T-1"), "T-1")).toEqual(NO_TABS);
  });

  it("ignores a task that is not open", () => {
    const open = tabs(["T-1"], "T-1");
    expect(closeTask(open, "T-9")).toBe(open);
  });
});
