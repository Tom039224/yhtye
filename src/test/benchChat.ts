// Content and in-page helpers for the long-chat benchmark (scripts/bench-long-chat.mjs). The layout
// harness (layoutHarness.tsx) takes its size from the URL (`?exchanges=1000`) and, with `?mix=real`,
// fills the conversation with what a real one holds (Markdown with headings / lists / code blocks /
// tables, tool calls, group cards, help and checkpoint notices, long answers) instead of the layout
// checks' few wide lines. Not a test; imported by the harness only.

import type { ApiEvent, ApiEventBody, AttachmentMeta, Group, Help, Task } from "../api/generated";
import type { AppStore } from "../store/app";
import { agentText, chunk, delivered, ev, turnEnded, userMessage } from "./events";
import type { FakeCore, MemoryTransport } from "./memoryTransport";

/** Round trips of the layout checks' conversation (`?scenario=long`). */
export const DEFAULT_EXCHANGES = 160;
/** More would only make the page unusable; the number is a typo then. */
export const MAX_EXCHANGES = 20_000;

/** Round trips asked by `?exchanges=` (a whole number from 1 up; anything else: the default). */
export function exchangesFrom(search: string): number {
  const asked = new URLSearchParams(search).get("exchanges");
  if (asked === null || !/^\d+$/.test(asked)) return DEFAULT_EXCHANGES;
  const n = Number(asked);
  return n >= 1 && n <= MAX_EXCHANGES ? n : DEFAULT_EXCHANGES;
}

export type Mix = "plain" | "real";

/** The conversation's content, `?mix=` (`real`: what real chats hold; default: the layout checks' lines). */
export function mixFrom(search: string): Mix {
  return new URLSearchParams(search).get("mix") === "real" ? "real" : "plain";
}

// ---- Markdown of the kind agents write ----------------------------------------------------------

const SECTIONS: ((i: number) => string)[] = [
  (i) => `## ${i} 回目の調査結果\n\n既存の実装を読んで、変更が必要な箇所を洗い出しました。\`store.set()\` は 1 回の呼び出しごとに購読者へ通知し、画面側は **全体** を作り直します。ストリーミング中はチャンクが届くたびにこれが起きるため、長い会話ほど 1 回の描画が重くなります。`,
  (i) => `### 変更の方針\n\n- 差分だけを描画するように \`memo\` を足す (${i})\n- 確定したメッセージの Markdown は一度だけ解析する\n- 無限アニメーションは \`prefers-reduced-motion\` で止める\n- 長い一覧は画面に見える分だけを描く\n- スクロール位置の読み取りは描画の後にまとめる`,
  (i) => `次のコードで確認できます。\n\n\`\`\`ts\nexport function applyLive(view: ProjectView, ev: ApiEvent): ProjectView {\n  const streaming = applyChunk(view.streaming, ev);\n  if (streaming) return streaming === view.streaming ? view : { ...view, streaming };\n  // item ${i}\n  return { ...view, transcripts: applyTranscriptEvent(view.transcripts, ev, view.state) };\n}\n\`\`\``,
  (i) => `| 指標 | 変更前 | 変更後 | 備考 |\n| --- | ---: | ---: | --- |\n| 描画 (ms) | ${40 + (i % 7)} | ${12 + (i % 3)} | 160 往復 |\n| 描画 (ms) | ${260 + (i % 11)} | ${14 + (i % 4)} | 1000 往復 |\n| メモリ (MB) | ${180 + (i % 13)} | ${150 + (i % 5)} | WebKitWebProcess |\n| ノード数 | ${9000 + i} | ${3200 + (i % 90)} | \`document.querySelectorAll("*")\` |`,
  (i) => `1. \`pnpm test\` を実行して既存のテストが通ることを確かめる\n2. \`pnpm check:layout\` で画面がはみ出していないことを確かめる\n3. 実機 (WebKitGTK) で入力欄の遅れを確かめる (${i})\n\n> 注意: ソフトウェア描画では、アニメーションの 1 フレームごとに画面全体の再合成が走ることがあります。`,
  (i) => `\`\`\`rust\nfn fold(events: &[Event], from: usize) -> State {\n    events.iter().skip(from).fold(State::default(), |s, e| s.apply(e)) // ${i}\n}\n\`\`\`\n\nRust 側は変更しません。ログの畳み込みは起動時に 1 度だけです。`,
];

/** Markdown of about `chars` characters: headings, lists, code blocks, tables and quotes in turn. */
export function markdownOf(seed: number, chars: number): string {
  const parts: string[] = [];
  let length = 0;
  for (let k = 0; length < chars; k++) {
    const part = SECTIONS[(seed + k) % SECTIONS.length](seed * 31 + k);
    parts.push(part);
    length += part.length + 2;
  }
  return parts.join("\n\n");
}

/** What an agent says in one turn, in the sizes real answers come in: short, medium, with a table, long. */
function answerOf(i: number): string {
  switch (i % 4) {
    case 0:
      return `了解しました (${i})。次の 3 点を確認してから進めます。\n\n- 現在のブランチの状態\n- 影響を受けるテスト\n- 設定ファイルの変更の有無`;
    case 1:
      return markdownOf(i, 700);
    case 2:
      return markdownOf(i, 1500);
    default:
      return markdownOf(i, 3800);
  }
}

function userTextOf(i: number): string {
  switch (i % 5) {
    case 0:
      return `${i} 件目: テストが落ちています。原因を調べて直してください。`;
    case 1:
      return `${i} 件目の依頼です。\nタスクを 3 つに分けて、並行して進めてください。ブランチ名は feature/bench-${i} にします。`;
    case 2:
      return `${i} 件目: このエラーが出ます。\n\n\`\`\`\nTypeError: Cannot read properties of undefined (reading 'seq')\n    at applyDurable (project.ts:141)\n    at AppStore.onEvent (app.ts:781)\n\`\`\``;
    default:
      return `${i} 件目。`;
  }
}

const TOOLS: { kind: string; title: (i: number) => string }[] = [
  { kind: "read", title: (i) => `Read src/store/app.ts (${i})` },
  { kind: "search", title: (i) => `Grep "applyLive" in src (${i})` },
  { kind: "edit", title: (i) => `Edit src/ui/TranscriptView.tsx (${i})` },
  { kind: "execute", title: (i) => `Run pnpm test -- --run src/store (${i})` },
  { kind: "fetch", title: (i) => `Fetch https://example.test/docs/${i}` },
];

/** One notice of the core to the orchestrator, as `inbox_queued` carries it. */
const NOTICE_KINDS = ["checkpoint_reached", "help_raised", "merge_result", "group_settled"] as const;

// ---- the conversation --------------------------------------------------------------------------

/** What `realLog` needs of the scenario around it (the harness' ids and templates). */
export interface RealLogOptions {
  exchanges: number;
  chat: string;
  /** The chat's orchestrator session key. */
  orchestrator: string;
  /** The group the scenario's tasks belong to; more are made from it every `GROUP_EVERY` round trips. */
  group: Group;
  /** The scenario's tasks (notices name them). */
  taskIds: string[];
  /** The task of a group made on the way. */
  task: Task;
  /** Files attached to the last message (the layout checks look at them). */
  attachments: AttachmentMeta[];
}

const GROUP_EVERY = 25;
const MAX_GROUPS = 12;

/** The durable events of a long, varied conversation (ids of the inbox and sequence numbers from 1). */
export function realLog(o: RealLogOptions): ApiEvent[] {
  const log: ApiEvent[] = [];
  let seq = 1;
  let inbox = 0;
  let groups = 0;
  const push = (make: (seq: number) => ApiEvent) => void log.push(make(seq++));
  const notice = (i: number, kind: (typeof NOTICE_KINDS)[number]) => {
    const id = ++inbox;
    const task = o.taskIds[i % o.taskIds.length];
    const body = markdownOf(i, 240);
    push((n) => ev(n, { type: "domain", event: { type: "inbox_queued", entry: { id, chat: o.chat, item: { kind, attrs: [["task", task], ["step", String(i % 3)]], body } } } }));
    push((n) => delivered(n, id, o.chat));
  };

  for (let i = 1; i <= o.exchanges; i++) {
    const id = ++inbox;
    push((n) => {
      const message = userMessage(n, id, userTextOf(i), o.chat);
      if (i === o.exchanges && message.body.type === "domain" && message.body.event.type === "inbox_queued") message.body.event.entry.item.attachments = o.attachments;
      return message;
    });
    push((n) => delivered(n, id, o.chat));
    push((n) => agentText(n, o.orchestrator, markdownOf(i + 3, 300), "thought"));
    for (let k = 0; k <= i % 4; k++) {
      const tool = TOOLS[(i + k) % TOOLS.length];
      const callId = `t${i}-${k}`;
      push((n) => ev(n, { type: "agent", session: o.orchestrator, event: { type: "output", data: { kind: "tool_call", update: { toolCallId: callId, title: tool.title(i), kind: tool.kind } } } }));
      push((n) => ev(n, { type: "agent", session: o.orchestrator, event: { type: "output", data: { kind: "tool_call_update", update: { toolCallId: callId, status: "completed" } } } }));
    }
    if (i % 5 === 0) {
      push((n) => ev(n, { type: "tool_called", record: { binding: { session: o.orchestrator, role: "orchestrator", project: "repo", chat: o.chat }, tool: "create_task", args: { title: `タスク ${i}` }, result: { Ok: { detail: markdownOf(i, 200) } } } }));
    }
    push((n) => agentText(n, o.orchestrator, answerOf(i)));
    if (i % GROUP_EVERY === 0 && groups < MAX_GROUPS) {
      groups++;
      const group: Group = { ...o.group, id: `G-${100 + groups}`, title: `${i} 往復目のグループ: 描画の見直し`, group_branch: `yhtye/G-${100 + groups}` };
      push((n) => ev(n, { type: "domain", event: { type: "group_created", group } }));
      for (let t = 0; t < 2; t++) {
        const task: Task = { ...o.task, id: `T-${100 + groups * 2 + t}`, group: group.id, title: `${i} 往復目のタスク ${t + 1}: Markdown の再解析を減らす` };
        push((n) => ev(n, { type: "domain", event: { type: "task_created", task } }));
      }
    }
    if (i % 9 === 0) notice(i, "checkpoint_reached");
    if (i % 15 === 0) notice(i, "help_raised");
    if (i % 20 === 0) notice(i, "merge_result");
    push((n) => turnEnded(n, o.orchestrator));
  }
  return log;
}

// ---- driving the page from the script ----------------------------------------------------------

/** What `scripts/bench-long-chat.mjs` calls in the page (`window.__yhtye.bench`). */
export interface BenchApi {
  /** The shown chat's orchestrator, and a task's session that is not on screen. */
  orchestrator: string;
  otherSession: string;
  /** Items in the shown chat's transcript, and the lowest `seq` loaded (1: the whole log). */
  items(): number;
  historyStart(): number;
  /** Loads every older page, as scrolling up to the start would; returns the pages read. */
  loadAllHistory(): Promise<number>;
  /**
   * Streams Markdown into `session` as live chunks, `intervalMs` after the last one was handled
   * (a busy page gets fewer, not a backlog), until `stopStream` or `maxMs`. Resolves when over.
   */
  startStream(session: string, intervalMs: number, maxMs: number): void;
  stopStream(): { emitted: number; chars: number; elapsedMs: number };
  /** Ends what was streamed to `session` as the core does: the whole text, durable. */
  settleStream(session: string): void;
  /** One durable event (or the pair of events a single occurrence is) of the shown chat. */
  inject(kind: InjectKind): void;
}

export type InjectKind = "long-answer" | "help" | "task-result";

const STREAM_TEXT = markdownOf(7, 12_000);
const CHUNK_CHARS = 9;

/** Builds the page-side API over the store and the in-memory core of the harness. */
export function benchApi(store: AppStore, core: FakeCore, transport: MemoryTransport, orchestrator: string, taskIds: string[]): BenchApi {
  let injected = 0;
  let stream: { session: string; sent: number; startedAt: number; timer: ReturnType<typeof setTimeout> | null; text: string } | null = null;
  const cursor = () => store.getState().project?.cursor ?? 0;
  const durable = (body: ApiEventBody) => {
    const event = ev(cursor() + 1, body);
    core.log.push(event);
    transport.emit(event);
  };
  const task = taskIds[1] ?? taskIds[0];
  const emitNotice = (kind: (typeof NOTICE_KINDS)[number], attrs: [string, string][], body: string) => {
    const id = 1_000_000 + ++injected;
    durable({ type: "domain", event: { type: "inbox_queued", entry: { id, chat: "C-1", item: { kind, attrs, body } } } });
    durable({ type: "domain", event: { type: "inbox_delivered", chat: "C-1", up_to: id } });
  };
  const stopTimer = () => {
    if (stream?.timer != null) clearTimeout(stream.timer);
  };
  return {
    orchestrator,
    otherSession: `${task}/implementer`,
    items: () => store.getState().project?.transcripts[orchestrator]?.length ?? 0,
    historyStart: () => store.getState().project?.historyStart ?? 1,
    async loadAllHistory() {
      let pages = 0;
      while ((store.getState().project?.historyStart ?? 1) > 1) {
        await store.loadOlderHistory();
        pages++;
      }
      return pages;
    },
    startStream(session, intervalMs, maxMs) {
      stopTimer();
      const state = { session, sent: 0, startedAt: performance.now(), timer: null as ReturnType<typeof setTimeout> | null, text: "" };
      stream = state;
      const next = () => {
        if (stream !== state) return;
        if (performance.now() - state.startedAt >= maxMs) return;
        const from = (state.sent * CHUNK_CHARS) % STREAM_TEXT.length;
        const piece = STREAM_TEXT.slice(from, from + CHUNK_CHARS);
        state.text += piece;
        state.sent++;
        transport.emit(chunk(cursor(), session, piece));
        state.timer = setTimeout(next, intervalMs);
      };
      next();
    },
    stopStream() {
      stopTimer();
      const last = stream;
      stream = null;
      return { emitted: last?.sent ?? 0, chars: last?.text.length ?? 0, elapsedMs: last ? performance.now() - last.startedAt : 0 };
    },
    settleStream(session) {
      store.flushLive();
      const text = store.getState().live.streaming[session]?.message ?? "";
      durable({ type: "agent_text", session, kind: "message", text: text || STREAM_TEXT.slice(0, 200) });
      if (session === orchestrator) durable({ type: "agent", session, event: { type: "turn_ended", data: { Ok: "end_turn" } } });
    },
    inject(kind) {
      const seed = ++injected;
      if (kind === "long-answer") {
        durable({ type: "agent_text", session: orchestrator, kind: "message", text: markdownOf(seed, 4500) });
        return;
      }
      if (kind === "help") {
        const help: Help = {
          id: `H-bench-${seed}`,
          task,
          step: 0,
          kind: "blocked",
          message: markdownOf(seed, 500),
          source: { by: "agent", role: "implementer" },
          state: "open",
          agent_lost: false,
          reply: null,
        };
        durable({ type: "domain", event: { type: "help_raised", help } });
        emitNotice("help_raised", [["task", task], ["help", help.id]], help.message);
        return;
      }
      const result = markdownOf(seed, 1800);
      durable({ type: "domain", event: { type: "step_completed", task, step: 0, result, verdict: null } });
      emitNotice("checkpoint_reached", [["task", task], ["step", "0"]], result.slice(0, 300));
    },
  };
}
