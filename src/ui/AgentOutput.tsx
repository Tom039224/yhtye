import { type ReactNode, useRef, useState } from "react";

import type { ProjectView } from "../store/project";
import { OlderHistory } from "./Conversation";
import { taskSessions } from "./taskInfo";
import { TranscriptView } from "./TranscriptView";
import { useAutoScroll } from "./useAutoScroll";

export { taskSessions };

/** The selected task's agent sessions (implementer, reviewers) and step results. */
export function AgentOutput({ view, task, tabs }: { view: ProjectView; task: string; tabs: ReactNode }) {
  const [chosen, setChosen] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const sessions = taskSessions(view, task);
  const session = chosen && sessions.includes(chosen) ? chosen : (sessions[sessions.length - 1] ?? null);
  const items = session ? view.transcripts[session] : undefined;
  const streaming = session ? view.streaming[session] : undefined;
  useAutoScroll(scroller, [items, streaming], items?.[0]?.seq);
  const results = view.state?.tasks.find((t) => t.id === task)?.steps.filter((s) => s.result) ?? [];

  return (
    <div className="agent-output" style={{ display: "contents" }}>
      <header className="bottom-header">
        {tabs}
        <span className="spacer" />
        {sessions.map((key) => {
          const record = view.sessions.find((s) => s.session_key === key);
          return (
            <button
              type="button"
              key={key}
              className={`session-tab ${key === session ? "active" : ""}`}
              onClick={() => setChosen(key)}
            >
              {key}
              {record ? ` · ${record.turn_running ? "working" : record.status}` : ""}
            </button>
          );
        })}
      </header>
      <div className="scroll" ref={scroller}>
        {results.length > 0 ? (
          <div className="transcript" aria-label="step results">
            {results.map((s, i) => (
              <div key={i} className="tool">
                <span className="tool-kind">{s.kind} result{s.verdict ? ` (${s.verdict})` : ""}:</span> {s.result}
              </div>
            ))}
          </div>
        ) : null}
        {!session ? (
          <p className="empty">{task} のエージェントはまだ起動していません。</p>
        ) : (
          <TranscriptView
            items={items ?? []}
            streaming={streaming}
            agentLabel={session.split("/")[1] ?? session}
            before={<OlderHistory view={view} />}
          />
        )}
      </div>
    </div>
  );
}
