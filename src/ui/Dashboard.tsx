import "./dashboard.css";

import { DashboardProjects } from "./DashboardProjects";
import { ExternalLink } from "./ExternalLink";
import { UsageStatsSection } from "./UsageStatsSection";

/** Where the usage statistics took their ideas from (credited in the dashboard's footer). */
export const AGENTSVIEW_URL = "https://github.com/kenn-io/agentsview";

/**
 * The dashboard (the first view, design §10): a welcome, the projects of every
 * host with "プロジェクトを追加", and the usage statistics of the host on
 * screen. Opening a project from it calls `onOpenWork` to switch to the work view.
 */
export function Dashboard({ onOpenWork }: { onOpenWork: () => void }) {
  return (
    <section className="dashboard" aria-label="dashboard">
      <div className="dashboard-scroll">
        <header className="dashboard-welcome">
          <h1 className="dashboard-title">ようこそ</h1>
          <p className="dashboard-lead">プロジェクトを選ぶか、git リポジトリを追加して、オーケストレータに依頼を始めましょう。</p>
        </header>
        <DashboardProjects onOpenWork={onOpenWork} />
        <UsageStatsSection />
        <footer className="dashboard-credit">
          利用統計は <ExternalLink href={AGENTSVIEW_URL}>agentsview</ExternalLink> (MIT) を参考にしています
        </footer>
      </div>
    </section>
  );
}
