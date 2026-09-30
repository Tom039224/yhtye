import { useEffect, useRef, useState } from "react";

import { AgentSettingsSection, type Scope } from "./AgentSettingsSection";
import { Icon, IconButton, type IconName } from "./Icon";
import { SecretEnvSection } from "./SecretEnvSection";

/** The sections of the settings. More are added here (each is a left-hand entry). */
const SECTIONS = [
  { id: "agents", label: "エージェント", icon: "bot" },
  { id: "secrets", label: "秘密の環境変数", icon: "key" },
] as const satisfies readonly { id: string; label: string; icon: IconName }[];
type SectionId = (typeof SECTIONS)[number]["id"];

interface Props {
  /** The open project (the "このプロジェクト" scope), if any. */
  project: { id: string; name: string } | null;
  onClose: () => void;
}

/**
 * The app-wide settings, a large modal over the conversation (Stage 7d):
 * sections on the left, their content on the right, and a toggle between all
 * projects (globe) and this one (folder) at the top. Esc or × closes it. Changes are saved as they are made.
 */
export function SettingsModal({ project, onClose }: Props) {
  const [section, setSection] = useState<SectionId>("agents");
  const [chosenScope, setScope] = useState<Scope>(project ? "project" : "global");
  const scope: Scope = project ? chosenScope : "global";
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      opener?.focus?.();
    };
  }, [onClose]);

  return (
    <div className="modal-backdrop">
      <div className="settings-modal" role="dialog" aria-modal="true" aria-label="設定" tabIndex={-1} ref={dialog}>
        <nav className="settings-nav" aria-label="設定の項目">
          <h2 className="settings-nav-title">
            <Icon name="sliders" size={16} />
            <span className="sr-only">設定</span>
          </h2>
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              className={`settings-nav-item ${section === s.id ? "active" : ""}`}
              aria-current={section === s.id ? "page" : undefined}
              onClick={() => setSection(s.id)}
            >
              <Icon name={s.icon} size={15} />
              {s.label}
            </button>
          ))}
        </nav>
        <div className="settings-main">
          <header className="settings-head">
            <h3 className="settings-title">{SECTIONS.find((s) => s.id === section)?.label}</h3>
            {section === "agents" ? (
              <div className="scope-toggle" role="group" aria-label="設定の範囲">
                <button type="button" aria-label="全体" title="全体 (すべてのプロジェクト)" aria-pressed={scope === "global"} onClick={() => setScope("global")}>
                  <Icon name="globe" size={14} />
                </button>
                <button
                  type="button"
                  aria-label={`このプロジェクト${project ? ` (${project.name})` : ""}`}
                  aria-pressed={scope === "project"}
                  disabled={!project}
                  title={project ? `このプロジェクト (${project.name})` : "プロジェクトを開いていません"}
                  onClick={() => setScope("project")}
                >
                  <Icon name="folder" size={14} />
                  {project ? <span className="scope-name">{project.name}</span> : null}
                </button>
              </div>
            ) : null}
            <span className="spacer" />
            <IconButton icon="x" label="閉じる" title="閉じる (Esc)" onClick={onClose} />
          </header>
          <div className="settings-body">
            {section === "agents" ? <AgentSettingsSection project={project} scope={scope} /> : <SecretEnvSection />}
          </div>
        </div>
      </div>
    </div>
  );
}
