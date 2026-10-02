import { type ReactNode, useRef, useState } from "react";

import type { ProjectView } from "../store/project";
import { OlderHistory } from "./Conversation";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { STEP_LABEL, stepIcon } from "./statusIcons";
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
  const owner = view.state?.tasks.find((t) => t.id === task);
  const results = owner?.steps.filter((s) => s.result) ?? [];

  return (
    <div className="agent-output" style={{ display: "contents" }}>
      <header className="bottom-header">
        {tabs}
        <span className="spacer" />
        {sessions.map((key) => {
          const record = view.sessions.find((s) => s.session_key === key);
          const state = record ? (record.turn_running ? "working" : record.status) : null;
          return (
            <button
              type="button"
              key={key}
              className={`session-tab ${key === session ? "active" : ""}`}
              aria-label={key}
              aria-pressed={key === session}
              title={state ? `${key} · ${state}` : key}
              onClick={() => setChosen(key)}
            >
              <span className={`status-dot ${dotClass(state)}`} aria-hidden="true" />
              {key.split("/")[1] ?? key}
            </button>
          );
        })}
      </header>
      <div className="scroll" ref={scroller}>
        {results.length > 0 ? (
          <div className="transcript" aria-label="step results">
            {results.map((s, i) => (
              <div key={i} className={`step-result ${s.verdict ? `step-result-${s.verdict}` : ""}`} title={`${STEP_LABEL[s.kind]}の結果`}>
                <Icon name={stepIcon(s.kind, owner?.kind ?? "code")} size={13} />
                {s.verdict ? <Icon name={s.verdict === "approve" ? "check" : "refresh"} size={12} className="step-result-verdict" /> : null}
                <span className="sr-only">
                  {s.kind} result{s.verdict ? ` (${s.verdict})` : ""}:
                </span>
                <span className="step-result-text">{s.result}</span>
              </div>
            ))}
          </div>
        ) : null}
        {!session ? (
          <EmptyState icon="bot" text={`${task} のエージェントは未起動です`} />
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

/** The session's state as the dot of the status shapes (idle green, working blue, ended hollow). */
function dotClass(state: string | null): string {
  if (state === "working") return "status-dot-working";
  return state === "live" ? "status-dot-idle" : "";
}
