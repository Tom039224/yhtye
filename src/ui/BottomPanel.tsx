import { useEffect, useState } from "react";

import type { ProjectView } from "../store/project";
import { AgentOutput } from "./AgentOutput";
import { GitPanel } from "./GitPanel";
import { Icon, IconButton } from "./Icon";

export type BottomTab = "git" | "output";

interface Props {
  view: ProjectView;
  selectedTask: string | null;
  tab: BottomTab;
  onTab: (tab: BottomTab) => void;
  onCloseTask: () => void;
}

/** Which tab the panel shows: selecting a task switches to its output. */
export function useBottomTab(selectedTask: string | null): [BottomTab, (tab: BottomTab) => void] {
  const [tab, setTab] = useState<BottomTab>("git");
  useEffect(() => {
    setTab(selectedTask ? "output" : "git");
  }, [selectedTask]);
  return [tab, setTab];
}

/** The task whose output the panel shows, or `null` while it shows git (the output gets the taller panel). */
export const outputTask = (tab: BottomTab, selectedTask: string | null): string | null => (tab === "output" ? selectedTask : null);

/**
 * The design's git panel (§3.6), which also hosts the selected task's agent
 * output (the design has no place for it; Stage 4's view is kept reachable
 * here). The tab is held by the workspace, which sizes the panel per tab.
 */
export function BottomPanel({ view, selectedTask, tab, onTab, onCloseTask }: Props) {
  const output = outputTask(tab, selectedTask);
  return (
    <section className={`bottom ${output ? "tall" : ""}`} aria-label={output ? "agent output" : "git"}>
      {output ? (
        <AgentOutput
          view={view}
          task={output}
          tabs={<Tabs tab={tab} task={selectedTask} onTab={onTab} onClose={onCloseTask} />}
        />
      ) : (
        <GitPanel view={view} tabs={<Tabs tab={tab} task={selectedTask} onTab={onTab} onClose={onCloseTask} />} />
      )}
    </section>
  );
}

function Tabs({
  tab,
  task,
  onTab,
  onClose,
}: {
  tab: BottomTab;
  task: string | null;
  onTab: (t: BottomTab) => void;
  onClose: () => void;
}) {
  return (
    <>
      <button
        type="button"
        className={`tab ${tab === "git" ? "active" : ""}`}
        aria-label="git"
        aria-pressed={tab === "git"}
        title="git のグラフ"
        onClick={() => onTab("git")}
      >
        <Icon name="git-branch" size={14} />
      </button>
      {task ? (
        <>
          <button
            type="button"
            className={`tab ${tab === "output" ? "active" : ""}`}
            aria-label={`${task} の出力`}
            aria-pressed={tab === "output"}
            title={`${task} のエージェントの出力`}
            onClick={() => onTab("output")}
          >
            <Icon name="terminal" size={14} />
            {task}
          </button>
          <IconButton icon="x" size={14} label="出力を閉じる" onClick={onClose} />
        </>
      ) : null}
    </>
  );
}
