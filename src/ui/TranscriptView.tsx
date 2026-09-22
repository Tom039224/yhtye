import type { TranscriptItem } from "../store/transcript";
import { formatTime } from "./labels";

interface Props {
  items: TranscriptItem[];
  streaming?: { message: string; thought: string };
  /** Label for the agent's messages ("orchestrator", "implementer", ...). */
  agentLabel: string;
  /** Undelivered inbox entry ids (user messages still queued). */
  queued?: ReadonlySet<number>;
}

export function TranscriptView({ items, streaming, agentLabel, queued }: Props) {
  return (
    <div className="transcript">
      {items.map((item, i) => (
        // Transcripts only grow (tool calls update in place), so the index is stable.
        <Item key={i} item={item} agentLabel={agentLabel} queued={queued} />
      ))}
      {streaming?.thought ? <Thought text={streaming.thought} streaming /> : null}
      {streaming?.message ? (
        <div className="msg msg-agent" data-testid="streaming">
          <div className="msg-label msg-label-agent">{agentLabel.toUpperCase()} · …</div>
          <div className="msg-body">{streaming.message}</div>
        </div>
      ) : null}
    </div>
  );
}

function Thought({ text, streaming }: { text: string; streaming?: boolean }) {
  return (
    <details className="thought">
      <summary>{streaming ? "thinking…" : "thought"}</summary>
      <div className="thought-body">{text}</div>
    </details>
  );
}

function Item({ item, agentLabel, queued }: { item: TranscriptItem; agentLabel: string; queued?: ReadonlySet<number> }) {
  switch (item.kind) {
    case "user": {
      const waiting = queued?.has(item.inboxId) ?? false;
      return (
        <div className="msg msg-user">
          <div className="msg-label">
            YOU · {formatTime(item.ts)}
            {waiting ? <span className="queued"> · 待機中 (オーケストレータの手が空いたら届く)</span> : null}
          </div>
          <div className="msg-body">{item.text}</div>
        </div>
      );
    }
    case "notice":
      return (
        <div className="notice">
          <span className="mono">yhtye → {agentLabel}: {item.inboxKind}</span>
          {item.attrs.length > 0 ? <span className="mono dim"> {item.attrs.map(([k, v]) => `${k}=${v}`).join(" ")}</span> : null}
          {item.body ? <div className="notice-body">{item.body}</div> : null}
        </div>
      );
    case "prompt":
      return (
        <details className="prompt">
          <summary className="mono">prompt · {formatTime(item.ts)}</summary>
          <div className="prompt-body">{item.text}</div>
        </details>
      );
    case "text":
      return item.textKind === "thought" ? (
        <Thought text={item.text} />
      ) : (
        <div className="msg msg-agent">
          <div className="msg-label msg-label-agent">
            {agentLabel.toUpperCase()} · {formatTime(item.ts)}
          </div>
          <div className="msg-body">{item.text}</div>
        </div>
      );
    case "tool":
      return (
        <div className="tool mono">
          <span className="tool-kind">tool</span> {item.title}
          <span className={`tool-status tool-status-${item.status ?? "pending"}`}> · {item.status ?? "pending"}</span>
        </div>
      );
    case "yhtye_tool":
      return (
        <details className={`tool mono ${item.ok ? "" : "error-text"}`}>
          <summary>
            <span className="tool-kind">yhtye</span> {item.tool}
            {item.by === "user" ? " (you)" : ""} · {item.ok ? "ok" : "error"}
          </summary>
          <div className="tool-detail">{item.detail}</div>
        </details>
      );
    case "turn":
      return <div className={`turn mono ${item.error ? "error-text" : "dim"}`}>turn ended: {item.outcome}</div>;
    case "lifecycle":
      return <div className={`lifecycle mono ${item.error ? "error-text" : "dim"}`}>{item.text}</div>;
  }
}
