import { type FormEvent, useState } from "react";

import { useAppState, useStore } from "../store/useStore";

export function ProjectPicker() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const current = useAppState((s) => s.project?.info.id ?? null);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const [path, setPath] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (path.trim()) void store.openProject(path.trim());
  };

  return (
    <section className="panel projects" aria-label="projects">
      <header className="panel-header section-title">PROJECTS</header>
      {projects.length === 0 ? <p className="empty">まだプロジェクトがありません。</p> : null}
      <ul className="project-list">
        {projects.map((p) => (
          <li key={p.id}>
            <button
              type="button"
              className={`project ${p.id === current ? "project-current" : ""}`}
              title={p.path}
              disabled={!connected || opening}
              onClick={() => void store.openProject(p.path)}
            >
              <span className={`ring ${p.open ? "ring-open" : ""}`} />
              {p.name}
            </button>
          </li>
        ))}
      </ul>
      <form className="open-form" onSubmit={submit}>
        <label className="section-title" htmlFor="project-path">
          OPEN
        </label>
        <input
          id="project-path"
          className="mono"
          placeholder="/path/to/git/repository"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          disabled={!connected || opening}
        />
        <button type="submit" className="btn" disabled={!connected || opening || !path.trim()}>
          {opening ? "開いています…" : "開く"}
        </button>
      </form>
    </section>
  );
}
