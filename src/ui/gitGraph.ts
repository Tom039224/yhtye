// Lane layout of the git panel (docs/design/orchestrator-desktop.md §3.6).
// Commits arrive newest first in topological order (`git log --topo-order`);
// each commit gets a lane, lanes stay where they are while they live (no
// compaction, so lines are straight), and every row records which lanes pass
// through, which end in the commit and where its parents continue.

import type { GitCommit } from "../api/generated";

export interface GraphRow {
  commit: GitCommit;
  /** Lane of the commit. */
  lane: number;
  /** Lanes that end in this commit (its children above), including `lane` when it had one. */
  incoming: number[];
  /** Lanes the commit's parents continue in (first parent first). */
  outgoing: number[];
  /**
   * Lanes passing by in the top half / bottom half of the row. A lane that
   * the commit's parent line merges into stays in both (it runs on through
   * the row); `outgoing` only adds the lines that start at the commit.
   */
  throughTop: number[];
  throughBottom: number[];
  /**
   * A lane that ends by joining another one in the bottom half (the first
   * parent was expected further right: the lower lane keeps it, so the base
   * branch's line stays straight).
   */
  joins: { from: number; to: number }[];
  /** Lane identity (a new id per lane allocation), for colouring, by lane index. */
  ids: Map<number, number>;
}

export interface Graph {
  rows: GraphRow[];
  /** Most lanes in use at once (width of the drawing). */
  width: number;
  /** The first commit seen on each lane id (its tip within the loaded range). */
  tips: Map<number, GitCommit>;
}

function freeSlot(lanes: (string | null)[], taken: number): number {
  const i = lanes.findIndex((l, n) => l === null && n !== taken);
  return i >= 0 ? i : lanes.length;
}

export function layoutGraph(commits: GitCommit[]): Graph {
  let lanes: (string | null)[] = [];
  let ids: number[] = [];
  let nextId = 0;
  let width = 0;
  const rows: GraphRow[] = [];
  const tips = new Map<number, GitCommit>();

  for (const commit of commits) {
    const before = lanes;
    const beforeIds = ids;
    const incoming = before.flatMap((sha, i) => (sha === commit.sha ? [i] : []));
    const lane = incoming[0] ?? freeSlot(before, -1);
    const after = [...before];
    const afterIds = [...beforeIds];
    for (const i of incoming) after[i] = null;
    if (incoming.length === 0) afterIds[lane] = nextId++;
    const laneId = afterIds[lane];
    if (!tips.has(laneId)) tips.set(laneId, commit);

    const outgoing: number[] = [];
    const joins: { from: number; to: number }[] = [];
    commit.parents.forEach((parent, n) => {
      const existing = after.indexOf(parent);
      if (n === 0 && existing > lane) {
        after[existing] = null;
        after[lane] = parent;
        joins.push({ from: existing, to: lane });
        outgoing.push(lane);
        return;
      }
      if (existing >= 0) {
        outgoing.push(existing);
        return;
      }
      const slot = n === 0 ? lane : freeSlot(after, lane);
      after[slot] = parent;
      if (n !== 0) afterIds[slot] = nextId++;
      outgoing.push(slot);
    });
    // A first parent already expected by a lane to the left: this lane ends in it.

    const throughTop = before.flatMap((sha, i) => (sha !== null && sha !== commit.sha ? [i] : []));
    // A lane that was already waiting for a parent of this commit keeps running
    // through the row (the commit's own line only joins it at the bottom edge).
    const throughBottom = before.flatMap((sha, i) => (sha !== null && sha !== commit.sha && after[i] === sha ? [i] : []));
    const joined = joins.map((j) => j.from);
    const rowIds = new Map<number, number>();
    for (const i of [...throughTop, ...throughBottom, ...incoming, ...outgoing, ...joined, lane]) {
      const id = afterIds[i] ?? beforeIds[i];
      if (id !== undefined) rowIds.set(i, id);
    }
    rowIds.set(lane, laneId);
    width = Math.max(width, before.length, after.length, lane + 1);
    rows.push({ commit, lane, incoming, outgoing, throughTop, throughBottom, joins, ids: rowIds });

    while (after.length > 0 && after[after.length - 1] === null) {
      after.pop();
      afterIds.pop();
    }
    lanes = after;
    ids = afterIds;
  }
  return { rows, width, tips };
}
