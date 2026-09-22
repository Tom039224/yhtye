import { type ReactNode, useMemo } from "react";

import type { GitCommit, GitOverview, State } from "../api/generated";
import type { ProjectView } from "../store/project";
import { useAppState, useStore } from "../store/useStore";
import { type Graph, layoutGraph } from "./gitGraph";
import { type Tone, TONE_LABEL, taskTone } from "./labels";
import { focusGroup } from "./taskInfo";

/** Lane x positions and row height of the design (§3.6: 19 / 41 / 63 px, 28 px rows). */
const LANE_X0 = 19;
const LANE_DX = 22;
const ROW_H = 28;
const MID = ROW_H / 2;
const laneX = (lane: number) => LANE_X0 + lane * LANE_DX;

/** Colour family of a branch: the task's status tone, or base / other. */
type BranchTone = Tone | "base" | "other";

interface BranchLabel {
  text: string;
  tone: BranchTone;
}

/** `yhtye/G-1-T-2` → T-2's tone; `yhtye/G-1` → the group; the base branch → base. */
function branchLabel(name: string, state: State | null, base: string | null): BranchLabel {
  if (name === base) return { text: name, tone: "base" };
  const task = /^yhtye\/(G-\d+)-(T-\d+)$/.exec(name);
  if (task && state) {
    const t = state.tasks.find((x) => x.id === task[2]);
    if (t) {
      const tone = taskTone(t);
      return { text: `${t.id} ${TONE_LABEL[tone]}`, tone };
    }
  }
  const group = /^yhtye\/(G-\d+)$/.exec(name);
  if (group) return { text: group[1], tone: "other" };
  return { text: name, tone: "other" };
}

function laneClass(tone: BranchTone | undefined): string {
  switch (tone) {
    case "implementing":
    case "reviewing":
    case "handling":
    case "done":
      return tone;
    case "base":
    case undefined:
      return "base";
    default:
      return "implementing";
  }
}

const LANE_STROKE: Record<string, string> = {
  base: "var(--git-lane-base)",
  implementing: "var(--git-lane-implementing)",
  reviewing: "var(--git-lane-reviewing)",
  handling: "var(--git-lane-handling)",
  done: "var(--git-lane-done)",
};
const NODE_STROKE: Record<string, string> = {
  base: "var(--git-node-base)",
  implementing: "var(--git-node-implementing)",
  reviewing: "var(--git-node-reviewing)",
  handling: "var(--git-node-handling)",
  done: "var(--git-node-done)",
};

/** The git panel (§3.6): the repository's recent commits of all local branches. */
export function GitPanel({ view, tabs }: { view: ProjectView; tabs: ReactNode }) {
  const store = useStore();
  const git = useAppState((s) => (s.git?.project === view.info.id ? s.git : null));
  const overview = git?.overview ?? null;
  const state = view.state;
  const group = state ? focusGroup(state) : null;
  const base = group?.base_branch ?? overview?.head ?? null;
  const graph = useMemo(() => (overview ? layoutGraph(overview.commits) : null), [overview]);

  return (
    <>
      <header className="bottom-header">
        {tabs}
        <span className="bottom-meta">
          {group ? `${group.group_branch} ← ${group.base_branch}` : (overview?.head ?? "")}
        </span>
        <span className="spacer" />
        {git?.error ? <span className="bottom-meta error-text" title={git.error}>読み込み失敗</span> : null}
        <button type="button" className="tab" title="git を読み直す" onClick={() => void store.refreshGit()} disabled={git?.loading}>
          {git?.loading ? "…" : "graph ↻"}
        </button>
      </header>
      <div className="scroll">
        {!overview ? (
          <p className="empty">{git?.error ? `git を読めませんでした: ${git.error}` : "読み込み中…"}</p>
        ) : overview.commits.length === 0 ? (
          <p className="empty">コミットはまだありません。</p>
        ) : graph ? (
          <GraphRows graph={graph} overview={overview} state={state} base={base} />
        ) : null}
      </div>
    </>
  );
}

function GraphRows({ graph, overview, state, base }: { graph: Graph; overview: GitOverview; state: State | null; base: string | null }) {
  // A lane's colour comes from the branch at its tip (its newest commit here).
  // The base branch wins: its line keeps the neutral colour even when task
  // branches point at the same commit (they have not moved yet).
  const commitTone = (c: GitCommit): BranchTone | undefined => {
    const labels = c.branches.map((b) => branchLabel(b, state, base));
    return (labels.find((l) => l.tone === "base") ?? labels.find((l) => l.tone !== "other") ?? labels[0])?.tone;
  };
  const laneTone = new Map<number, BranchTone>();
  for (const [id, tip] of graph.tips) laneTone.set(id, commitTone(tip) ?? "base");
  const toneAt = (row: Graph["rows"][number], lane: number) => laneClass(laneTone.get(row.ids.get(lane) ?? -1));
  const width = laneX(Math.max(graph.width, 3) - 1) + 23;

  return (
    <div className="graph" data-testid="git-graph">
      {graph.rows.map((row) => {
        const c = row.commit;
        const isHead = c.sha === overview.head_sha;
        // A commit with a branch of its own is drawn in that branch's colour.
        const own = commitTone(c);
        const tone = own ? laneClass(own) : toneAt(row, row.lane);
        const x = laneX(row.lane);
        return (
          <div key={c.sha} className="graph-row" style={{ paddingLeft: width }} title={`${c.sha}\n${c.subject}`}>
            <svg width={width} aria-hidden="true">
              {row.throughTop.map((l) => (
                <line key={`t${l}`} x1={laneX(l)} y1={0} x2={laneX(l)} y2={MID} stroke={LANE_STROKE[toneAt(row, l)]} strokeWidth={1.5} />
              ))}
              {row.incoming.map((l) => (
                <line key={`i${l}`} x1={laneX(l)} y1={0} x2={x} y2={MID} stroke={LANE_STROKE[toneAt(row, l)]} strokeWidth={1.5} />
              ))}
              {row.throughBottom.map((l) => (
                <line key={`b${l}`} x1={laneX(l)} y1={MID} x2={laneX(l)} y2={ROW_H} stroke={LANE_STROKE[toneAt(row, l)]} strokeWidth={1.5} />
              ))}
              {row.joins.map((j) => (
                <line key={`j${j.from}`} x1={laneX(j.from)} y1={MID} x2={laneX(j.to)} y2={ROW_H} stroke={LANE_STROKE[toneAt(row, j.from)]} strokeWidth={1.5} />
              ))}
              {row.outgoing.map((l) => (
                <line key={`o${l}`} x1={x} y1={MID} x2={laneX(l)} y2={ROW_H} stroke={LANE_STROKE[toneAt(row, l)]} strokeWidth={1.5} />
              ))}
              <circle
                cx={x}
                cy={MID}
                r={4.5}
                fill={isHead ? NODE_STROKE[tone] : "var(--surface-inset)"}
                stroke={isHead && tone === "base" ? "var(--git-node-base-head)" : NODE_STROKE[tone]}
                strokeWidth={2}
              />
            </svg>
            <span className="graph-sha">{c.sha.slice(0, 7)}</span>
            <span className="graph-subject">{c.subject}</span>
            <Badges commit={c} isHead={isHead} state={state} base={base} head={overview.head} />
          </div>
        );
      })}
      {overview.truncated ? <p className="empty">これより前のコミットは省略しています。</p> : null}
    </div>
  );
}

function Badges({ commit, isHead, state, base, head }: { commit: GitCommit; isHead: boolean; state: State | null; base: string | null; head: string | null }) {
  if (commit.branches.length === 0 && !isHead) return null;
  return (
    <span className="graph-badges">
      {isHead ? <span className="git-badge git-badge-head">HEAD{head ? "" : " (detached)"}</span> : null}
      {commit.branches.map((b) => {
        const label = branchLabel(b, state, base);
        const cls = label.tone === "base" || label.tone === "other" || label.tone === "waiting" || label.tone === "cancelled" ? "" : `git-badge-${label.tone}`;
        return (
          <span key={b} className={`git-badge ${cls}`} title={b}>
            {label.text}
          </span>
        );
      })}
    </span>
  );
}
