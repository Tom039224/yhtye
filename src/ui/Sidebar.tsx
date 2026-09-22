import { type FormEvent, useState } from "react";

import type { ProjectInfo } from "../api/generated";
import type { AppState } from "../store/app";
import type { ProjectView } from "../store/project";
import { useAppState, useStore } from "../store/useStore";
import { useNow } from "./useNow";

type Ring = "running" | "ready" | "stopped";

/** Running: the open project's orchestrator or one of its agents is in a turn. */
function projectRing(p: ProjectInfo, view: ProjectView | null): Ring {
  if (!p.open) return "stopped";
  if (view && view.info.id === p.id && view.sessions.some((s) => s.status === "live" && s.turn_running)) return "running";
  return "ready";
}

const RING_LABEL: Record<Ring, string> = { running: "実行中", ready: "待機中", stopped: "停止" };

/** PROJECTS / BRANCHES / core host (design §3.3), all from the core. */
export function Sidebar() {
  return (
    <aside className="sidebar" aria-label="projects">
      <Projects />
      <Branches />
      <HostFooter />
    </aside>
  );
}

function Projects() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const view = useAppState((s) => s.project);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const [path, setPath] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (path.trim()) void store.openProject(path.trim());
  };
  return (
    <>
      <div className="section-title">PROJECTS</div>
      {projects.length === 0 ? <p className="side-note">まだプロジェクトがありません。</p> : null}
      <ul className="side-list">
        {projects.map((p) => {
          const ring = projectRing(p, view);
          return (
            <li key={p.id}>
              <button
                type="button"
                className={`side-row ${p.id === view?.info.id ? "current" : ""}`}
                title={`${p.path} · ${RING_LABEL[ring]}`}
                disabled={!connected || opening}
                onClick={() => void store.openProject(p.path)}
              >
                <span className={`ring ring-${ring}`} data-testid={`ring-${p.id}`} />
                <span className="name">{p.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <form className="open-form" onSubmit={submit}>
        <input
          aria-label="開くリポジトリのパス"
          placeholder="/path/to/git/repository"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          disabled={!connected || opening}
        />
        <button type="submit" className="btn btn-small" disabled={!connected || opening || !path.trim()}>
          {opening ? "…" : "開く"}
        </button>
      </form>
    </>
  );
}

function selectBranches(s: AppState) {
  return s.git && s.project && s.git.project === s.project.info.id ? s.git : null;
}

function Branches() {
  const git = useAppState(selectBranches);
  const overview = git?.overview;
  return (
    <>
      <div className="section-title branches-title">BRANCHES</div>
      {!git ? <p className="side-note">プロジェクトを開くと表示します。</p> : null}
      {git && !overview && git.error ? <p className="side-note error-text">{git.error}</p> : null}
      {overview && overview.branches.length === 0 ? <p className="side-note">ブランチはまだありません。</p> : null}
      {overview ? (
        <ul className="side-list" aria-label="branches">
          {overview.branches.map((b) => {
            const current = b.name === overview.head;
            return (
              <li key={b.name} className={`side-row branch-row ${current ? "current" : ""}`} title={`${b.name} · ${b.sha.slice(0, 7)}`}>
                <span className={`dot ${current ? "dot-ok" : ""}`} />
                <span className="name">{b.name}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </>
  );
}

/**
 * The design's "remote host" footer. Yhtye runs one local core; this shows the
 * connection to it and when it last sent an event. Remote hosts are not
 * supported (Stage 6b decides whether they come).
 */
function HostFooter() {
  const connection = useAppState((s) => s.connection);
  const transport = useAppState((s) => s.transport);
  const lastEventAt = useAppState((s) => s.lastEventAt);
  const now = useNow(1000);
  const dot = connection.state === "open" ? "dot-ok" : connection.state === "connecting" ? "dot-busy" : "dot-bad";
  const ago = lastEventAt === null ? null : Math.max(0, Math.round((now - lastEventAt) / 1000));
  return (
    <div className="side-footer" data-testid="host">
      <div className="host">
        <span className={`dot ${dot}`} />
        local core · {transport.kind === "tauri" ? "app" : transport.kind}
      </div>
      <div className="host-sync">{ago === null ? "no events yet" : `last event ${formatAgo(ago)}`}</div>
    </div>
  );
}

function formatAgo(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}
