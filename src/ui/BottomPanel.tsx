import type { ProjectView } from "../store/project";
import { AgentOutput } from "./AgentOutput";
import type { BottomTabs } from "./bottomTabs";
import { GitPanel } from "./GitPanel";
import { Icon, IconButton } from "./Icon";

interface Props {
  view: ProjectView;
  tabs: BottomTabs;
  /** Shows git (`null`) or an open task's output. */
  onShow: (active: string | null) => void;
  onClose: (task: string) => void;
}

/**
 * The design's git panel (§3.6), which also hosts the agent output of the tasks
 * that were opened (the design has no place for it; Stage 4's view is kept
 * reachable here): a git tab, then one tab per open task with its own close
 * button. The tabs are held by the workspace, which sizes the panel per tab.
 */
export function BottomPanel({ view, tabs, onShow, onClose }: Props) {
  const header = <Tabs tabs={tabs} onShow={onShow} onClose={onClose} />;
  return (
    <section className={`bottom ${tabs.active ? "tall" : ""}`} aria-label={tabs.active ? "agent output" : "git"}>
      {tabs.active ? <AgentOutput key={tabs.active} view={view} task={tabs.active} tabs={header} /> : <GitPanel view={view} tabs={header} />}
    </section>
  );
}

function Tabs({ tabs, onShow, onClose }: Pick<Props, "tabs" | "onShow" | "onClose">) {
  return (
    <div className="bottom-tabs">
      <button
        type="button"
        className={`tab ${tabs.active === null ? "active" : ""}`}
        aria-label="git"
        aria-pressed={tabs.active === null}
        title="git のグラフ"
        onClick={() => onShow(null)}
      >
        <Icon name="git-branch" size={14} />
      </button>
      {tabs.tasks.map((task) => {
        const active = tabs.active === task;
        return (
          <span key={task} className={`tab-group ${active ? "active" : ""}`}>
            <button
              type="button"
              className="tab"
              aria-label={`${task} の出力`}
              aria-pressed={active}
              title={`${task} のエージェントの出力`}
              onClick={() => onShow(task)}
            >
              <Icon name="terminal" size={14} />
              {task}
            </button>
            <IconButton className="tab-close" icon="x" size={12} label={`${task} の出力を閉じる`} onClick={() => onClose(task)} />
          </span>
        );
      })}
    </div>
  );
}
