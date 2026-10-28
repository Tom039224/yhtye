import { describe, expect, it } from "vitest";

import { dropOnto, fillSlots, sortByOrder, step, toFront } from "./order";

describe("sortByOrder", () => {
  it("puts listed items first in list order and the rest after, as they were", () => {
    const items = ["n1", "b", "n2", "a"];
    expect(sortByOrder(items, ["a", "gone", "b"], (s) => s)).toEqual(["a", "b", "n1", "n2"]);
    expect(sortByOrder(items, [], (s) => s)).toEqual(items);
  });
});

describe("list edits", () => {
  it("moves a key to the front, adding it when missing", () => {
    expect(toFront(["a", "b", "c"], "c")).toEqual(["c", "a", "b"]);
    expect(toFront(["a"], "z")).toEqual(["z", "a"]);
  });

  it("drops a key onto another: it takes the place, the ones between shift", () => {
    expect(dropOnto(["a", "b", "c", "d"], "a", "c")).toEqual(["b", "c", "a", "d"]);
    expect(dropOnto(["a", "b", "c", "d"], "d", "b")).toEqual(["a", "d", "b", "c"]);
    expect(dropOnto(["a", "b"], "a", "a")).toEqual(["a", "b"]);
    expect(dropOnto(["a", "b"], "a", "zz")).toEqual(["a", "b"]);
  });

  it("steps a key up or down by one and refuses past the ends", () => {
    expect(step(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
    expect(step(["a", "b", "c"], "b", 1)).toEqual(["a", "c", "b"]);
    expect(step(["a", "b", "c"], "a", -1)).toBeNull();
    expect(step(["a", "b", "c"], "c", 1)).toBeNull();
    expect(step(["a"], "zz", 1)).toBeNull();
  });

  it("puts a shown subset back into the places it had", () => {
    expect(fillSlots(["a", "x", "b", "y", "c"], ["c", "a", "b"])).toEqual(["c", "x", "a", "y", "b"]);
    expect(fillSlots(["a", "b"], [])).toEqual(["a", "b"]);
  });
});
