import { useRef, useState } from "react";

import type { ProjectView } from "../store/project";
import { TranscriptView } from "./TranscriptView";
import { useAutoScroll } from "./useAutoScroll";

/** Session keys of a task (`T-1/implementer`, `T-1/review-2`, ...) in start order. */
export function taskSessions(view: ProjectView, task: string): string[] {
  const prefix = `${task}/`;
  const keys = new Set<string>();
  for (const key of Object.keys(view.transcripts)) if (key.startsWith(prefix)) keys.add(key);
  for (const s of view.sessions) if (s.session_key.startsWith(prefix)) keys.add(s.session_key);
  const firstSeq = (k: string) => view.transcripts[k]?.[0]?.seq ?? Number.MAX_SAFE_INTEGER;
  return [...keys].sort((a, b) => firstSeq(a) - firstSeq(b));
}

export function AgentOutput({ view, task }: { view: ProjectView; task: string | null }) {
  const [chosen, setChosen] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const sessions = task ? taskSessions(view, task) : [];
  const session = chosen && sessions.includes(chosen) ? chosen : (sessions[sessions.length - 1] ?? null);
  const items = session ? view.transcripts[session] : undefined;
  const streaming = session ? view.streaming[session] : undefined;
  useAutoScroll(scroller, [items, streaming]);

  return (
    <section className="panel agent-output" aria-label="agent output">
      <header className="panel-header">
        <span>agent output</span>
        {sessions.map((key) => {
          const record = view.sessions.find((s) => s.session_key === key);
          return (
            <button
              type="button"
              key={key}
              className={`tab mono ${key === session ? "tab-active" : ""}`}
              onClick={() => setChosen(key)}
            >
              {key}
              {record ? ` · ${record.turn_running ? "working" : record.status}` : ""}
            </button>
          );
        })}
      </header>
      <div className="scroll" ref={scroller}>
        {!task ? (
          <p className="empty">タスク名をクリックすると、そのエージェントの出力を表示します。</p>
        ) : !session ? (
          <p className="empty">{task} のエージェントはまだ起動していません。</p>
        ) : (
          <TranscriptView items={items ?? []} streaming={streaming} agentLabel={session.split("/")[1] ?? session} />
        )}
      </div>
    </section>
  );
}
