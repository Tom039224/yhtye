import { describe, expect, it } from "vitest";

import { memoryPrefs } from "../store/prefs";
import {
  BOTTOM_MIN,
  bottomBounds,
  clampSize,
  dragSize,
  KEY_STEP,
  KEY_STEP_LARGE,
  keyboardSize,
  LAYOUT_KEY,
  loadLayout,
  parseLayout,
  RIGHT_MIN,
  rightBounds,
  SIDEBAR_MAX,
  SIDEBAR_MIN,
  saveSize,
  sidebarBounds,
  TASKS_MIN,
  withSize,
} from "./layout";

describe("clampSize", () => {
  it("keeps a value inside the limits and returns limits it goes beyond", () => {
    expect(clampSize(200, { min: 100, max: 300 })).toBe(200);
    expect(clampSize(50, { min: 100, max: 300 })).toBe(100);
    expect(clampSize(999, { min: 100, max: 300 })).toBe(300);
  });

  it("lets the room win when it is smaller than the minimum, so nothing overflows", () => {
    expect(clampSize(200, { min: 400, max: 300 })).toBe(300);
  });
});

describe("bounds", () => {
  it("limits the sidebar so the conversation and the right column keep their minimum", () => {
    // 1180 = the window's minimum width: 1180 - 44 (rail) - 380 - 420 = 336.
    expect(sidebarBounds(1180)).toEqual({ min: SIDEBAR_MIN, max: 336 });
    expect(sidebarBounds(1440)).toEqual({ min: SIDEBAR_MIN, max: SIDEBAR_MAX });
  });

  it("never lets the sidebar maximum fall below its minimum on a tiny container", () => {
    expect(sidebarBounds(500)).toEqual({ min: SIDEBAR_MIN, max: SIDEBAR_MIN });
  });

  it("leaves the conversation its minimum width beside the right column", () => {
    expect(rightBounds(1182)).toEqual({ min: RIGHT_MIN, max: 1182 - 380 });
    expect(rightBounds(600)).toEqual({ min: RIGHT_MIN, max: RIGHT_MIN });
  });

  it("leaves the task list its minimum height above the bottom panel", () => {
    expect(bottomBounds(842)).toEqual({ min: BOTTOM_MIN, max: 842 - TASKS_MIN });
    expect(bottomBounds(200)).toEqual({ min: BOTTOM_MIN, max: BOTTOM_MIN });
  });

  it("only applies the static limits while the container is not laid out", () => {
    for (const unknown of [undefined, 0, Number.NaN]) {
      expect(sidebarBounds(unknown).max).toBe(SIDEBAR_MAX);
      expect(rightBounds(unknown).min).toBe(RIGHT_MIN);
      expect(bottomBounds(unknown).min).toBe(BOTTOM_MIN);
      expect(rightBounds(unknown).max).toBeGreaterThan(1000);
    }
  });
});

describe("dragSize", () => {
  const bounds = { min: 100, max: 300 };

  it("grows a panel before the handle as the pointer moves forward", () => {
    expect(dragSize(200, 30, "before", bounds)).toBe(230);
    expect(dragSize(200, -30, "before", bounds)).toBe(170);
  });

  it("grows a panel after the handle as the pointer moves back", () => {
    expect(dragSize(200, 30, "after", bounds)).toBe(170);
    expect(dragSize(200, -30, "after", bounds)).toBe(230);
  });

  it("stops at the limits however far the pointer goes", () => {
    expect(dragSize(200, 5000, "before", bounds)).toBe(300);
    expect(dragSize(200, -5000, "before", bounds)).toBe(100);
    expect(dragSize(200, 5000, "after", bounds)).toBe(100);
  });

  it("rounds to whole pixels", () => {
    expect(dragSize(200, 10.6, "before", bounds)).toBe(211);
  });
});

describe("keyboardSize", () => {
  const base = { current: 200, bounds: { min: 100, max: 300 } };

  it("moves a vertical handle with the left and right arrows", () => {
    const o = { ...base, orientation: "vertical", side: "before" } as const;
    expect(keyboardSize("ArrowRight", false, o)).toBe(200 + KEY_STEP);
    expect(keyboardSize("ArrowLeft", false, o)).toBe(200 - KEY_STEP);
    expect(keyboardSize("ArrowUp", false, o)).toBeNull();
  });

  it("moves a horizontal handle with the up and down arrows", () => {
    const o = { ...base, orientation: "horizontal", side: "after" } as const;
    // The panel is below the handle: moving the boundary down makes it smaller.
    expect(keyboardSize("ArrowDown", false, o)).toBe(200 - KEY_STEP);
    expect(keyboardSize("ArrowUp", false, o)).toBe(200 + KEY_STEP);
    expect(keyboardSize("ArrowLeft", false, o)).toBeNull();
  });

  it("takes larger steps with Shift", () => {
    const o = { ...base, orientation: "vertical", side: "before" } as const;
    expect(keyboardSize("ArrowRight", true, o)).toBe(200 + KEY_STEP_LARGE);
  });

  it("goes to the limits with Home and End and stays inside them", () => {
    const o = { ...base, orientation: "vertical", side: "after" } as const;
    expect(keyboardSize("Home", false, o)).toBe(100);
    expect(keyboardSize("End", false, o)).toBe(300);
    expect(keyboardSize("ArrowLeft", true, { ...o, current: 290 })).toBe(300);
    expect(keyboardSize("ArrowRight", true, { ...o, current: 110 })).toBe(100);
  });

  it("ignores other keys", () => {
    expect(keyboardSize("a", false, { ...base, orientation: "vertical", side: "before" })).toBeNull();
  });
});

describe("parseLayout", () => {
  it("reads the stored sizes", () => {
    expect(parseLayout('{"sidebar":250,"right":600,"git":300,"output":400}')).toEqual({
      sidebar: 250,
      right: 600,
      git: 300,
      output: 400,
    });
  });

  it("is empty when nothing is stored or the text is not usable", () => {
    for (const raw of [null, "", "not json", "[]", "null", "42", '"text"']) {
      expect(parseLayout(raw)).toEqual({});
    }
  });

  it("drops values that are not positive numbers and unknown keys", () => {
    const raw = JSON.stringify({ sidebar: "wide", right: -5, git: null, output: Number.MAX_VALUE, extra: 300 });
    const sizes = parseLayout(raw);
    expect(sizes.sidebar).toBeUndefined();
    expect(sizes.right).toBeUndefined();
    expect(sizes.git).toBeUndefined();
    expect(sizes).not.toHaveProperty("extra");
    // A number is kept, held to the sanity ceiling.
    expect(sizes.output).toBeLessThanOrEqual(4000);
  });

  it("holds stored sizes to each panel's own limits", () => {
    expect(parseLayout('{"sidebar":5,"right":10,"git":1}')).toEqual({ sidebar: SIDEBAR_MIN, right: RIGHT_MIN, git: BOTTOM_MIN });
    expect(parseLayout('{"sidebar":9000}').sidebar).toBe(SIDEBAR_MAX);
  });

  it("rounds to whole pixels", () => {
    expect(parseLayout('{"sidebar":250.4}').sidebar).toBe(250);
  });
});

describe("withSize", () => {
  it("sets one size and keeps the others", () => {
    expect(withSize({ sidebar: 250 }, "right", 600)).toEqual({ sidebar: 250, right: 600 });
  });

  it("drops a size on null so the default applies again", () => {
    expect(withSize({ sidebar: 250, right: 600 }, "right", null)).toEqual({ sidebar: 250 });
  });

  it("does not change its input", () => {
    const sizes = { sidebar: 250 };
    withSize(sizes, "sidebar", 300);
    expect(sizes).toEqual({ sidebar: 250 });
  });

  it("holds the size to the panel's limits", () => {
    expect(withSize({}, "sidebar", 9000).sidebar).toBe(SIDEBAR_MAX);
  });
});

describe("saveSize / loadLayout", () => {
  it("stores the sizes in the preferences and reads them back", () => {
    const prefs = memoryPrefs();
    saveSize(prefs, "sidebar", 260);
    saveSize(prefs, "git", 310);
    expect(loadLayout(prefs)).toEqual({ sidebar: 260, git: 310 });
    expect(JSON.parse(prefs.get(LAYOUT_KEY) ?? "")).toEqual({ sidebar: 260, git: 310 });
  });

  it("re-reads before writing, so a copy made earlier does not overwrite newer sizes", () => {
    const prefs = memoryPrefs();
    const stale = loadLayout(prefs);
    saveSize(prefs, "right", 600);
    saveSize(prefs, "sidebar", 240);
    expect(stale).toEqual({});
    expect(loadLayout(prefs)).toEqual({ right: 600, sidebar: 240 });
  });

  it("forgets a size on null", () => {
    const prefs = memoryPrefs({ [LAYOUT_KEY]: '{"sidebar":260,"right":600}' });
    expect(saveSize(prefs, "sidebar", null)).toEqual({ right: 600 });
    expect(loadLayout(prefs)).toEqual({ right: 600 });
  });

  it("starts from nothing when the stored text is damaged", () => {
    const prefs = memoryPrefs({ [LAYOUT_KEY]: "{oops" });
    expect(loadLayout(prefs)).toEqual({});
    expect(saveSize(prefs, "right", 500)).toEqual({ right: 500 });
  });
});
