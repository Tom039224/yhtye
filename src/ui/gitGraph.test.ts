import { describe, expect, it } from "vitest";

import type { GitCommit } from "../api/generated";
import { layoutGraph } from "./gitGraph";

function c(sha: string, parents: string[], branches: string[] = []): GitCommit {
  return { sha, parents, branches, subject: sha, ts_ms: 0 };
}

describe("git graph layout", () => {
  it("keeps a linear history in one lane", () => {
    const g = layoutGraph([c("c", ["b"], ["main"]), c("b", ["a"]), c("a", [])]);
    expect(g.rows.map((r) => r.lane)).toEqual([0, 0, 0]);
    expect(g.width).toBe(1);
    expect(g.rows[0].incoming).toEqual([]);
    expect(g.rows[1].incoming).toEqual([0]);
    expect(g.rows[2].outgoing).toEqual([]);
  });

  it("opens a lane for a merged branch and closes it at the fork point", () => {
    // m merges s (side) into main; s and b both come from a.
    const g = layoutGraph([c("m", ["b", "s"], ["main"]), c("s", ["a"], ["yhtye/G-1"]), c("b", ["a"]), c("a", [])]);
    const [m, s, b, a] = g.rows;
    expect(m.outgoing).toEqual([0, 1]);
    expect(s.lane).toBe(1);
    expect(s.throughTop).toEqual([0]);
    // s continues in its lane to a; b stays in lane 0.
    expect(s.outgoing).toEqual([1]);
    expect(b.lane).toBe(0);
    expect(b.throughTop).toEqual([1]);
    // Both wait for a: the side lane joins main's lane, which stays straight.
    expect(b.outgoing).toEqual([0]);
    expect(b.joins).toEqual([{ from: 1, to: 0 }]);
    expect(b.throughBottom).toEqual([]);
    expect(a.lane).toBe(0);
    expect(a.incoming).toEqual([0]);
    expect(g.width).toBe(2);
    expect(g.tips.get(s.ids.get(1) ?? -1)?.sha).toBe("s");
  });

  it("gives unmerged branch tips their own lanes", () => {
    const g = layoutGraph([c("t2", ["g"], ["yhtye/G-1-T-2"]), c("t1", ["g"], ["yhtye/G-1-T-1"]), c("g", ["a"], ["yhtye/G-1"]), c("a", [], ["main"])]);
    expect(g.rows.map((r) => r.lane)).toEqual([0, 1, 0, 0]);
    expect(g.rows[1].throughTop).toEqual([0]);
    // t1's parent g is already expected by lane 0: t1's line joins it.
    expect(g.rows[1].outgoing).toEqual([0]);
    expect(g.rows[2].incoming).toEqual([0]);
  });
});
