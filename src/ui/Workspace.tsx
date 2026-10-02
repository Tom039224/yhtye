import { useEffect, useState } from "react";

import type { ProjectView } from "../store/project";
import { BottomPanel } from "./BottomPanel";
import { Conversation } from "./Conversation";
import { addMention, type Mention, removeMention } from "./mentions";
import { TaskColumn } from "./TaskColumn";

/** The open project: conversation | tasks over git / agent output (design §2). */
export function Workspace({ view }: { view: ProjectView }) {
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  // Mentions belong to the chat whose composer they are in.
  const [mentionsByChat, setMentionsByChat] = useState<Record<string, Mention[]>>({});
  const chat = view.selectedChat ?? "";
  const mentions = mentionsByChat[chat] ?? [];
  const setMentions = (f: (m: Mention[]) => Mention[]) => setMentionsByChat((all) => ({ ...all, [chat]: f(all[chat] ?? []) }));
  // A selected task belongs to the previous chat's column.
  useEffect(() => setSelectedTask(null), [chat]);
  return (
    <div className="workspace">
      <Conversation
        view={view}
        mentions={mentions}
        onRemoveMention={(task) => setMentions((m) => removeMention(m, task))}
        onClearMentions={() => setMentions(() => [])}
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
