import { describe, expect, it } from "vitest";

import type { GitCommit } from "../api/generated";
import { type Graph, layoutGraph } from "./gitGraph";

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

  it("keeps the lane an ending lane merges into running through the row", () => {
    // t10 sits in lane 1 and ends in m1, which lane 0 already waits for:
    // lane 0 must not stop in the middle of t10's row.
    const g = layoutGraph([c("m2", ["m1", "t10"], ["main"]), c("t10", ["m1"], ["t10"]), c("m1", ["a"]), c("a", [])]);
    const t10 = g.rows[1];
    expect(t10.lane).toBe(1);
    expect(t10.throughTop).toEqual([0]);
    expect(t10.outgoing).toEqual([0]);
    expect(t10.throughBottom).toEqual([0]);
  });
});

/** Every line is continuous: the lanes leaving a row are the ones entering the next, and none stops in the middle. */
function expectConnected(g: Graph) {
  g.rows.forEach((row, i) => {
    for (const l of row.throughTop) {
      const continues = row.throughBottom.includes(l) || row.joins.some((j) => j.from === l);
      expect(continues, `${row.commit.sha}: lane ${l} stops in the middle of the row`).toBe(true);
    }
    const next = g.rows[i + 1];
    if (!next) return;
    const bottom = [...new Set([...row.throughBottom, ...row.outgoing])].sort();
    const top = [...new Set([...next.throughTop, ...next.incoming])].sort();
    expect(top, `${row.commit.sha} → ${next.commit.sha}`).toEqual(bottom);
  });
}

/** `sha|parents|branches`, newest first: the merge-heavy top of this repository's own history. */
const GROUP_HISTORY = `
78c61b7|70c90d5 1023a87|support/devin
1023a87|9c1825f 99840da|yhtye/G-2
99840da|9c1825f|yhtye/G-2-T-10
9c1825f|4b43880 edd2bb2|
edd2bb2|4b43880|yhtye/G-2-T-8
4b43880|79fef8a 2ae164c|
2ae164c|cc4ab5b|yhtye/G-2-T-6
79fef8a|cc4ab5b d9addbb|
d9addbb|cc4ab5b|yhtye/G-2-T-7
cc4ab5b|70c90d5 b31f3c0|
b31f3c0|70c90d5|yhtye/G-2-T-5
63ec00e|70c90d5 313d940|style/improve-icon yhtye/G-3
313d940|c185ab5 a35ef6e|yhtye/G-1
a35ef6e|c185ab5|yhtye/G-1-T-3
c185ab5|1e4a7c1 848e6b0|
848e6b0|70c90d5|yhtye/G-1-T-1
1e4a7c1|70c90d5 5c353ed|
5c353ed|70c90d5|yhtye/G-1-T-2
70c90d5|1a0e566|main
1a0e566|0f81a48|
0f81a48|c82c8ba|
c82c8ba|c753f9c 485776f|
485776f|c753f9c|
c753f9c|b39a738|
b39a738|4cac8ae|
4cac8ae||
`;

function parseHistory(text: string): GitCommit[] {
  return text
    .trim()
    .split("\n")
    .map((line) => {
      const [sha, parents, branches] = line.split("|");
      return c(sha, parents.split(" ").filter(Boolean), branches.split(" ").filter(Boolean));
    });
}

describe("git graph continuity", () => {
  it("never stops a lane in the middle of a row (merges of branches that share a fork point)", () => {
    expectConnected(layoutGraph(parseHistory(GROUP_HISTORY)));
  });

  it("stays connected when a merge joins a lane further right", () => {
    expectConnected(layoutGraph([c("m", ["b", "s"], ["main"]), c("s", ["a"]), c("b", ["a"]), c("a", [])]));
  });

  it("stays connected with unmerged tips that share a parent", () => {
    expectConnected(layoutGraph([c("t2", ["g"]), c("t1", ["g"]), c("g", ["a"]), c("a", [])]));
  });
});
