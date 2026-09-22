import { useMemo, useRef } from "react";

import { orchestratorSession, type ProjectView } from "../store/project";
import { ORCHESTRATOR } from "../store/transcript";
import { useAppState, useStore } from "../store/useStore";
import { Composer } from "./Composer";
import { TranscriptView } from "./TranscriptView";
import { useAutoScroll } from "./useAutoScroll";

export function Conversation({ view }: { view: ProjectView }) {
  const store = useStore();
  const connection = useAppState((s) => s.connection);
  const sending = useAppState((s) => s.busy.sending);
  const items = view.transcripts[ORCHESTRATOR];
  const streaming = view.streaming[ORCHESTRATOR];
  const queued = useMemo(() => new Set((view.state?.inbox ?? []).map((e) => e.id)), [view.state?.inbox]);
  const session = orchestratorSession(view);
  const scroller = useRef<HTMLDivElement>(null);
  useAutoScroll(scroller, [items, streaming]);

  const disabledReason =
    connection.state !== "open"
      ? "コアに接続していません"
      : view.phase === "loading"
        ? "読み込み中…"
        : view.phase === "error"
          ? "プロジェクトを読み込めませんでした"
          : null;

  return (
    <section className="panel conversation" aria-label="orchestrator">
      <header className="panel-header">
        <span>orchestrator</span>
        <SessionBadge status={session?.status} running={session?.turn_running ?? false} />
      </header>
      <div className="scroll" ref={scroller}>
        {items?.length || streaming ? (
          <TranscriptView items={items ?? []} streaming={streaming} agentLabel="orchestrator" queued={queued} />
        ) : (
          <p className="empty">まだ会話はありません。下の欄からオーケストレータに依頼してください。</p>
        )}
      </div>
      <Composer
        disabled={disabledReason !== null}
        disabledReason={disabledReason}
        sending={sending}
        turnRunning={session?.turn_running ?? false}
        onSend={(text) => store.sendMessage(text)}
        onCancel={() => void store.cancelTurn()}
      />
    </section>
  );
}

function SessionBadge({ status, running }: { status: string | undefined; running: boolean }) {
  if (!status) return <span className="badge mono dim">not started</span>;
  const label = status === "live" ? (running ? "working" : "idle") : status;
  const tone = status === "live" ? (running ? "tone-implementing" : "tone-done") : "tone-handling";
  return <span className={`badge mono ${tone}`} data-testid="orchestrator-status">{label}</span>;
}
