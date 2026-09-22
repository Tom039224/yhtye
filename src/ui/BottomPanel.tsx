import { useEffect, useState } from "react";

import type { ProjectView } from "../store/project";
import { AgentOutput } from "./AgentOutput";
import { GitPanel } from "./GitPanel";

interface Props {
  view: ProjectView;
  selectedTask: string | null;
  onCloseTask: () => void;
}

/**
 * The design's git panel (§3.6), which also hosts the selected task's agent
 * output (the design has no place for it; Stage 4's view is kept reachable
 * here). Selecting a task switches to its output.
 */
export function BottomPanel({ view, selectedTask, onCloseTask }: Props) {
  const [tab, setTab] = useState<"git" | "output">("git");
  useEffect(() => {
    setTab(selectedTask ? "output" : "git");
  }, [selectedTask]);
  const showOutput = tab === "output" && selectedTask !== null;
  return (
    <section className={`bottom ${showOutput ? "tall" : ""}`} aria-label={showOutput ? "agent output" : "git"}>
      {showOutput ? (
        <AgentOutput
          view={view}
          task={selectedTask}
          tabs={<Tabs tab={tab} task={selectedTask} onTab={setTab} onClose={onCloseTask} />}
        />
      ) : (
        <GitPanel view={view} tabs={<Tabs tab={tab} task={selectedTask} onTab={setTab} onClose={onCloseTask} />} />
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
  tab: "git" | "output";
  task: string | null;
  onTab: (t: "git" | "output") => void;
  onClose: () => void;
}) {
  return (
    <>
      <button type="button" className={`tab ${tab === "git" ? "active" : ""}`} onClick={() => onTab("git")}>
        git
      </button>
      {task ? (
        <>
          <button type="button" className={`tab ${tab === "output" ? "active" : ""}`} onClick={() => onTab("output")}>
            {task} output
          </button>
          <button type="button" className="tab" aria-label="出力を閉じる" title="出力を閉じる" onClick={onClose}>
            ×
          </button>
        </>
      ) : null}
    </>
  );
}
