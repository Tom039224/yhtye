import { useEffect, useRef, useState } from "react";

import { AgentSettingsSection, type Scope } from "./AgentSettingsSection";
import { HarnessSettingsSection } from "./HarnessSettingsSection";
import { SecretEnvSection } from "./SecretEnvSection";

/** The sections of the settings. More are added here (each is a left-hand entry). */
const SECTIONS = [
  { id: "agents", label: "エージェント" },
  { id: "harnesses", label: "ハーネス" },
  { id: "secrets", label: "秘密の環境変数" },
] as const;
type SectionId = (typeof SECTIONS)[number]["id"];

interface Props {
  /** The open project (the "このプロジェクト" scope), if any. */
  project: { id: string; name: string } | null;
  onClose: () => void;
}

/**
 * The app-wide settings, a large modal over the conversation (Stage 7d):
 * sections on the left, their content on the right, and a 全体 / このプロジェクト
 * toggle at the top. Esc or × closes it. Changes are saved as they are made.
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
          <h2 className="settings-nav-title">設定</h2>
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              className={`settings-nav-item ${section === s.id ? "active" : ""}`}
              aria-current={section === s.id ? "page" : undefined}
              onClick={() => setSection(s.id)}
            >
              {s.label}
            </button>
          ))}
        </nav>
        <div className="settings-main">
          <header className="settings-head">
            <h3 className="settings-title">{SECTIONS.find((s) => s.id === section)?.label}</h3>
            {section === "agents" ? (
            <div className="scope-toggle" role="group" aria-label="設定の範囲">
              <button type="button" aria-pressed={scope === "global"} onClick={() => setScope("global")}>
                全体
              </button>
              <button
                type="button"
                aria-pressed={scope === "project"}
                disabled={!project}
                title={project ? project.name : "プロジェクトを開いていません"}
                onClick={() => setScope("project")}
              >
                このプロジェクト{project ? ` (${project.name})` : ""}
              </button>
            </div>
            ) : null}
            <span className="spacer" />
            <button type="button" className="icon-btn" aria-label="閉じる" title="閉じる (Esc)" onClick={onClose}>
              ×
            </button>
          </header>
          <div className="settings-body">
            {section === "agents" ? (
              <AgentSettingsSection project={project} scope={scope} />
            ) : section === "harnesses" ? (
              <HarnessSettingsSection />
            ) : (
              <SecretEnvSection />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
