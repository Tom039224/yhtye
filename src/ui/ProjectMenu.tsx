import type { RefObject } from "react";

import type { ProjectInfo } from "../api/generated";
import { useAppState, useStore } from "../store/useStore";
import { canSleepProject } from "./projectActivity";
import { RowMenu } from "./RowMenu";

interface ProjectMenuProps {
  id: string;
  project: ProjectInfo;
  /** The element the menu sits under (the project's row, chip or the picker button). */
  anchor: RefObject<HTMLElement | null>;
  /** Asked to close; `refocus` says whether focus goes back to the anchor. */
  onClose: (refocus: boolean) => void;
  /** The user chose an action (the menu closes too). */
  onChosen?: () => void;
}

/**
 * The context menu of a project (a right click on its chip, on its row in the
 * picker, or on the picker button), in the chat menu's look. Sleep stops every
 * chat's orchestrator; it is shown but not choosable while nothing is awake or
 * while a chat is working (see `canSleepProject`).
 */
export function ProjectMenu({ id, project, anchor, onClose, onChosen }: ProjectMenuProps) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  return (
    <RowMenu
      id={id}
      label={`${project.name} の操作`}
      anchor={anchor}
      actions={[
        {
          id: "sleep",
          label: "スリープ",
          icon: "moon",
          disabled: !canSleepProject(project, view),
          onSelect: () => {
            void store.sleepProject(project.id);
            onChosen?.();
          },
        },
      ]}
      onClose={onClose}
    />
  );
}
