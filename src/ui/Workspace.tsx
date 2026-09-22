import { useState } from "react";

import type { ProjectView } from "../store/project";
import { BottomPanel } from "./BottomPanel";
import { Conversation } from "./Conversation";
import { addMention, type Mention, removeMention } from "./mentions";
import { TaskColumn } from "./TaskColumn";

/** The open project: conversation | tasks over git / agent output (design §2). */
export function Workspace({ view }: { view: ProjectView }) {
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [mentions, setMentions] = useState<Mention[]>([]);
  return (
    <div className="workspace">
      <Conversation
        view={view}
        mentions={mentions}
        onRemoveMention={(task) => setMentions((m) => removeMention(m, task))}
        onClearMentions={() => setMentions([])}
      />
      <div className="right">
        <TaskColumn
          view={view}
          selectedTask={selectedTask}
          onSelectTask={setSelectedTask}
          onMention={(m) => setMentions((list) => addMention(list, m))}
        />
        <BottomPanel view={view} selectedTask={selectedTask} onCloseTask={() => setSelectedTask(null)} />
      </div>
    </div>
  );
}
