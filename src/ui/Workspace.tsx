import { useState } from "react";

import type { ProjectView } from "../store/project";
import { AgentOutput } from "./AgentOutput";
import { Conversation } from "./Conversation";
import { TaskPanel } from "./TaskPanel";

/** The open project: conversation on the left, tasks and agent output on the right. */
export function Workspace({ view }: { view: ProjectView }) {
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  return (
    <div className="workspace">
      <Conversation view={view} />
      <div className="right">
        <section className="panel tasks" aria-label="tasks">
          <header className="panel-header">
            <span>tasks</span>
            <span className="mono dim">{view.info.path}</span>
          </header>
          <div className="scroll">
            {view.phase === "loading" ? <p className="empty">読み込み中…</p> : null}
            {view.phase === "error" ? <p className="empty error-text">{view.loadError}</p> : null}
            {view.state ? (
              <TaskPanel state={view.state} selectedTask={selectedTask} onSelectTask={setSelectedTask} />
            ) : null}
          </div>
        </section>
        <AgentOutput view={view} task={selectedTask} />
      </div>
    </div>
  );
}
