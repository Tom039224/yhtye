import type { RefObject } from "react";

import type { ProjectInfo } from "../api/generated";
import { useAppState, useStore } from "../store/useStore";
import { canSleepProject } from "./projectActivity";
import { RowMenu } from "./RowMenu";
import { moveActions } from "./useSortable";

interface ProjectMenuProps {
  id: string;
  project: ProjectInfo;
  /** The element the menu sits under (the project's row in the rail). */
  anchor: RefObject<HTMLElement | null>;
  /** Asked to close; `refocus` says whether focus goes back to the anchor. */
  onClose: (refocus: boolean) => void;
  /**
   * Adds "move up / down" for the projects on screen (`keys`, in order): the
   * menu closes, `onMove` gets the new order and `onMoved` runs (focus goes
   * back to the row).
   */
  order?: { keys: string[]; onMove: (next: string[]) => void; onMoved?: () => void };
}

/**
 * The context menu of a project (a right click on its row in the rail), in the
 * chat menu's look. Sleep stops every chat's orchestrator; it is shown but not
 * choosable while nothing is awake or while a chat is working (see
 * `canSleepProject`). With `order`, the project can also be moved up or down
 * among the projects shown.
 */
export function ProjectMenu({ id, project, anchor, onClose, order }: ProjectMenuProps) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  return (
    <RowMenu
      id={id}
      label={`${project.name} の操作`}
      anchor={anchor}
      actions={[
        ...(order
          ? moveActions(order.keys, project.id, order.onMove).map((a) => ({
              ...a,
              onSelect: () => {
                a.onSelect();
                order.onMoved?.();
              },
            }))
          : []),
        {
          id: "sleep",
          label: "スリープ",
          icon: "moon",
          disabled: !canSleepProject(project, view),
          onSelect: () => {
            void store.sleepProject(project.id);
          },
        },
      ]}
      onClose={onClose}
    />
  );
}
