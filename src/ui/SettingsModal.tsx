import { useEffect, useRef, useState } from "react";

import { useActiveHostEntry, useHostsState } from "../store/useHosts";
import { AgentSettingsSection, type Scope } from "./AgentSettingsSection";
import { HarnessSettingsSection } from "./HarnessSettingsSection";
import { HostsSettingsSection, type PairLinkRequest } from "./HostsSettingsSection";
import { Icon, IconButton, type IconName } from "./Icon";
import { RemoteMark } from "./RemoteMark";
import { RemoteSettingsElsewhere, RemoteSettingsSection } from "./RemoteSettingsSection";
import { SecretEnvSection } from "./SecretEnvSection";

/** The sections of the settings. More are added here (each is a left-hand entry). */
const SECTIONS = [
  { id: "agents", label: "エージェント", icon: "bot" },
  { id: "harnesses", label: "ハーネス", icon: "terminal" },
  { id: "secrets", label: "秘密の環境変数", icon: "key" },
  { id: "remote", label: "リモート接続", icon: "phone" },
  { id: "hosts", label: "接続先", icon: "remote" },
] as const satisfies readonly { id: string; label: string; icon: IconName }[];
type SectionId = (typeof SECTIONS)[number]["id"];

/** The sections whose settings are the host's, not this PC's (they carry the name of a remote host on screen). */
const HOST_SIDE: readonly SectionId[] = ["agents", "harnesses", "secrets"];

interface Props {
  /** The open project (the "このプロジェクト" scope), if any. */
  project: { id: string; name: string } | null;
  /** A pairing link that arrived: the "接続先" section opens with it in the box. */
  pairLink?: PairLinkRequest | null;
  onClose: () => void;
}

/**
 * The app-wide settings, a large modal over the conversation (Stage 7d):
 * sections on the left, their content on the right, and a toggle between all
 * projects (globe) and this one (folder) at the top. Esc or × closes it. Changes are saved as they are made.
 *
 * What it changes is the host on screen's: with a remote host's project open,
 * its name stands by the heading, and the host's own remote access (which only
 * the host can run) is explained instead of shown. "接続先" is about this PC:
 * the hosts it connects to.
 */
export function SettingsModal({ project, pairLink = null, onClose }: Props) {
  const remoteAvailable = useHostsState((s) => s.remoteAvailable) ?? false;
  const host = useActiveHostEntry();
  const remoteName = host?.remote ? (host.name ?? host.key) : null;
  const sections = SECTIONS.filter((s) => s.id !== "hosts" || remoteAvailable);
  const [section, setSection] = useState<SectionId>(pairLink ? "hosts" : "agents");
  const [chosenScope, setScope] = useState<Scope>(project ? "project" : "global");
  const scope: Scope = project ? chosenScope : "global";
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (pairLink) setSection("hosts");
  }, [pairLink]);

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
          {sections.map((s) => (
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
            {remoteName && HOST_SIDE.includes(section) ? (
              <span className="settings-host" title={`この設定は ${remoteName} のものです (この PC のものではありません)`}>
                <RemoteMark entry={host ?? undefined} />
                <span className="settings-host-name">{remoteName}</span>
                <span>の設定</span>
              </span>
            ) : null}
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
            {section === "agents" ? (
              <AgentSettingsSection project={project} scope={scope} />
            ) : section === "harnesses" ? (
              <HarnessSettingsSection />
            ) : section === "secrets" ? (
              <SecretEnvSection />
            ) : section === "hosts" ? (
              <HostsSettingsSection request={pairLink} />
            ) : remoteName ? (
              <RemoteSettingsElsewhere hostName={remoteName} />
            ) : (
              <RemoteSettingsSection />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
