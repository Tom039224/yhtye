import { useEffect, useRef, useState } from "react";

import type { ProjectView } from "../store/project";
import { BottomPanel, outputTask, useBottomTab } from "./BottomPanel";
import { Conversation } from "./Conversation";
import { bottomBounds, rightBounds } from "./layout";
import { addMention, type Mention, removeMention } from "./mentions";
import { Resizer } from "./Resizer";
import { TaskColumn } from "./TaskColumn";
import { setSizeVar, sizeVar, useLayoutSizes } from "./useLayoutSizes";

const RIGHT_WIDTH = "--right-width";
const BOTTOM_HEIGHT = "--bottom-height";

/**
 * The open project: conversation | tasks over git / agent output (design §2).
 * The boundaries can be dragged; the sizes are kept (see layout.ts).
 */
export function Workspace({ view }: { view: ProjectView }) {
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  // Mentions belong to the chat whose composer they are in.
  const [mentionsByChat, setMentionsByChat] = useState<Record<string, Mention[]>>({});
  const chat = view.selectedChat ?? "";
  const mentions = mentionsByChat[chat] ?? [];
  const setMentions = (f: (m: Mention[]) => Mention[]) => setMentionsByChat((all) => ({ ...all, [chat]: f(all[chat] ?? []) }));
  // A selected task belongs to the previous chat's column.
  useEffect(() => setSelectedTask(null), [chat]);

  const { sizes, setSize } = useLayoutSizes();
  const [tab, setTab] = useBottomTab(selectedTask);
  // The git panel and the agent output are sized separately: the output wants more room.
  const bottomKey = outputTask(tab, selectedTask) === null ? "git" : "output";
  const workspace = useRef<HTMLDivElement>(null);
  const right = useRef<HTMLDivElement>(null);
  const bottom = () => right.current?.querySelector<HTMLElement>(":scope > .bottom") ?? null;
  return (
    <div className="workspace" ref={workspace} style={{ ...sizeVar(RIGHT_WIDTH, sizes.right), ...sizeVar(BOTTOM_HEIGHT, sizes[bottomKey]) }}>
      <Conversation
        view={view}
        mentions={mentions}
        onRemoveMention={(task) => setMentions((m) => removeMention(m, task))}
        onClearMentions={() => setMentions(() => [])}
      />
      <Resizer
        orientation="vertical"
        side="after"
        label="右パネルの幅"
        measure={() => right.current?.getBoundingClientRect().width ?? 0}
        bounds={() => rightBounds(workspace.current?.clientWidth)}
        onResize={(px) => setSizeVar(workspace.current, RIGHT_WIDTH, px)}
        onCommit={(px) => setSize("right", px)}
      />
      <div className="right" ref={right}>
        <TaskColumn
          view={view}
          selectedTask={selectedTask}
          onSelectTask={setSelectedTask}
          onMention={(m) => setMentions((list) => addMention(list, m))}
        />
        <Resizer
          orientation="horizontal"
          side="after"
          label={bottomKey === "output" ? "出力パネルの高さ" : "git パネルの高さ"}
          measure={() => bottom()?.getBoundingClientRect().height ?? 0}
          bounds={() => bottomBounds(right.current?.clientHeight)}
          onResize={(px) => setSizeVar(workspace.current, BOTTOM_HEIGHT, px)}
          onCommit={(px) => setSize(bottomKey, px)}
        />
        <BottomPanel
          view={view}
          selectedTask={selectedTask}
          tab={tab}
          onTab={setTab}
          onCloseTask={() => setSelectedTask(null)}
        />
      </div>
    </div>
  );
}
