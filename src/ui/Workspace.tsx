import { useRef, useState } from "react";

import type { ProjectView } from "../store/project";
import { useAppState } from "../store/useStore";
import { BottomPanel } from "./BottomPanel";
import { useBottomTabs } from "./bottomTabs";
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
 * It reads the view itself, so that the app around it (the title bar, the rail,
 * the status bar) does not render again with every event of the project.
 */
export function Workspace() {
  const view = useAppState((s) => s.project);
  return view ? <ProjectWorkspace view={view} /> : null;
}

/**
 * The boundaries can be dragged; the sizes are kept (see layout.ts).
 */
function ProjectWorkspace({ view }: { view: ProjectView }) {
  // Mentions belong to the chat whose composer they are in.
  const [mentionsByChat, setMentionsByChat] = useState<Record<string, Mention[]>>({});
  const chat = view.selectedChat ?? "";
  const mentions = mentionsByChat[chat] ?? [];
  const setMentions = (f: (m: Mention[]) => Mention[]) => setMentionsByChat((all) => ({ ...all, [chat]: f(all[chat] ?? []) }));

  const { sizes, setSize } = useLayoutSizes();
  // The open task tabs belong to the chat's column: they start over with another chat.
  const { tabs, open, show, close } = useBottomTabs(chat);
  // The git panel and the agent output are sized separately: the output wants more room.
  const bottomKey = tabs.active === null ? "git" : "output";
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
          selectedTask={tabs.active}
          onSelectTask={open}
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
        <BottomPanel view={view} tabs={tabs} onShow={show} onClose={close} />
      </div>
    </div>
  );
}
