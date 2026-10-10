import type { ProjectView } from "../store/project";
import { AgentOutput } from "./AgentOutput";
import type { BottomTabs } from "./bottomTabs";
import { GitPanel } from "./GitPanel";
import { Icon, IconButton } from "./Icon";
import { ScrollStrip } from "./ScrollStrip";

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
 *
 * Once a task is open the tabs get a row of their own above the content's header (the git
 * heading, the session pick-list): they scroll sideways there however many there are, and
 * squeeze nothing. Until then git is the only tab and sits in the git header, as the design has it.
 */
export function BottomPanel({ view, tabs, onShow, onClose }: Props) {
  const hasTasks = tabs.tasks.length > 0;
  const gitTab = <GitTab active={tabs.active === null} onShow={onShow} />;
  return (
    <section className={`bottom ${tabs.active ? "tall" : ""}`} aria-label={tabs.active ? "agent output" : "git"}>
      {hasTasks ? (
        <ScrollStrip className="bottom-tabs" active={tabs.active}>
          {gitTab}
          {tabs.tasks.map((task) => (
            <TaskTab key={task} task={task} active={tabs.active === task} onShow={onShow} onClose={onClose} />
          ))}
        </ScrollStrip>
      ) : null}
      {tabs.active ? <AgentOutput key={tabs.active} view={view} task={tabs.active} /> : <GitPanel view={view} lead={hasTasks ? null : gitTab} />}
    </section>
  );
}

function GitTab({ active, onShow }: { active: boolean; onShow: Props["onShow"] }) {
  return (
    <button type="button" className={`tab ${active ? "active" : ""}`} aria-label="git" aria-pressed={active} title="git のグラフ" onClick={() => onShow(null)}>
      <Icon name="git-branch" size={14} />
    </button>
  );
}

function TaskTab({ task, active, onShow, onClose }: { task: string; active: boolean; onShow: Props["onShow"]; onClose: Props["onClose"] }) {
  return (
    <span className={`tab-group ${active ? "active" : ""}`}>
      <button type="button" className="tab" aria-label={`${task} の出力`} aria-pressed={active} title={`${task} のエージェントの出力`} onClick={() => onShow(task)}>
        <Icon name="terminal" size={14} />
        {task}
      </button>
      <IconButton className="tab-close" icon="x" size={12} label={`${task} の出力を閉じる`} onClick={() => onClose(task)} />
    </span>
  );
}
