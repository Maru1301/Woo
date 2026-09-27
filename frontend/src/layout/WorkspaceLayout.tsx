import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import type { RepositoryInfo } from "../lib/repository";

export type WorkspaceView = "history" | "changes" | "branches" | "tags" | "stashes" | "merge";
type DetailPosition = "right" | "bottom";
type SizePreset = "balanced" | "history" | "detail";

export function WorkspaceLayout({ repository, repositoryError, activeView, onViewChange, onOpen, openDisabled, remoteControls, workspaceName, workspaceMenu, repositoryTabs, activity, sidebar, changeCount, busy, children }: {
  repository: RepositoryInfo | null;
  repositoryError?: string;
  activeView: WorkspaceView;
  onViewChange: (view: WorkspaceView) => void;
  onOpen: () => void;
  openDisabled?: boolean;
  remoteControls?: ReactNode;
  workspaceName: string;
  workspaceMenu: ReactNode;
  repositoryTabs: ReactNode;
  activity: (open: boolean) => ReactNode;
  sidebar?: ReactNode;
  changeCount: number;
  busy: string | null;
  children: ReactNode;
}) {
  const [detailPosition, setDetailPosition] = useState<DetailPosition>("right");
  const [sizePreset, setSizePreset] = useState<SizePreset>("balanced");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [activityOpen, setActivityOpen] = useState(false);
  useEffect(() => {
    const dismiss = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (!target.closest(".woo-workspace-selector")) setWorkspaceOpen(false);
      if (!target.closest(".woo-activity-anchor")) setActivityOpen(false);
      if (!target.closest(".woo-settings")) setSettingsOpen(false);
    };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { setWorkspaceOpen(false); setActivityOpen(false); setSettingsOpen(false); } };
    document.addEventListener("mousedown", dismiss);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("mousedown", dismiss); document.removeEventListener("keydown", escape); };
  }, []);
  const masterSize = detailPosition === "right"
    ? { balanced: "58%", history: "72%", detail: "38%" }[sizePreset]
    : { balanced: "43%", history: "60%", detail: "27%" }[sizePreset];
  const navigation = (view: WorkspaceView, label: string, count?: number) =>
    <button type="button" className={activeView === view ? "active" : ""} aria-current={activeView === view ? "page" : undefined} onClick={() => onViewChange(view)}>{label}{count !== undefined && <span className="woo-count">{count}</span>}</button>;

  return <main className="app-shell woo-shell">
    <header className="topbar woo-topbar">
      <div className="brand"><span className="brand-mark">W</span><span>Woo</span></div>
      <div className="woo-workspace-selector">
        <button type="button" className="secondary" aria-expanded={workspaceOpen} onClick={() => { setWorkspaceOpen((value) => !value); setActivityOpen(false); setSettingsOpen(false); }}>Workspace · {workspaceName} ▾</button>
        {workspaceOpen && <div className="woo-workspace-menu">{workspaceMenu}</div>}
      </div>
      <div className="woo-toolbar-spacer" />
      {remoteControls}
      <div className="woo-activity-anchor">
        <button type="button" className="secondary" aria-label="Activity" aria-expanded={activityOpen} onClick={() => { setActivityOpen((value) => !value); setWorkspaceOpen(false); setSettingsOpen(false); }}>Activity</button>
        {activityOpen && <div className="woo-activity-popover">{activity(true)}</div>}
      </div>
      <div className="woo-settings">
        <button type="button" className="secondary woo-settings-button" aria-label="Layout settings" aria-expanded={settingsOpen} onClick={() => { setSettingsOpen((value) => !value); setWorkspaceOpen(false); setActivityOpen(false); }}>Settings</button>
        {settingsOpen && <div className="woo-settings-menu">
          <p>DETAIL POSITION</p>
          <button className={detailPosition === "right" ? "active" : ""} onClick={() => { setDetailPosition("right"); setSettingsOpen(false); }}>Right</button>
          <button className={detailPosition === "bottom" ? "active" : ""} onClick={() => { setDetailPosition("bottom"); setSettingsOpen(false); }}>Bottom</button>
          <p>SIZE PRESET</p>
          <button className={sizePreset === "balanced" ? "active" : ""} onClick={() => { setSizePreset("balanced"); setSettingsOpen(false); }}>Balanced</button>
          <button className={sizePreset === "history" ? "active" : ""} onClick={() => { setSizePreset("history"); setSettingsOpen(false); }}>History focused</button>
          <button className={sizePreset === "detail" ? "active" : ""} onClick={() => { setSizePreset("detail"); setSettingsOpen(false); }}>Detail focused</button>
        </div>}
      </div>
    </header>
    <div className="woo-tabs" aria-label="Workspace repositories">{repositoryTabs}<button type="button" className="woo-tab-add" onClick={onOpen} disabled={openDisabled} title="Add repository to workspace">+</button></div>
    {repository && (repositoryError || repository.watchWarning) && <p className="error woo-repository-alert" role="alert">{repositoryError || `Auto refresh unavailable: ${repository.watchWarning}. Use Refresh to update repository state.`}</p>}
    <div className="woo-main">
      {repository && <aside className="woo-sidebar" aria-label="Repository navigation">
        <nav className="woo-nav">{navigation("changes", "Changes", changeCount)}{navigation("history", "History")}</nav>
        <p className="woo-sidebar-label">References</p>{sidebar}
        <p className="woo-sidebar-label">Tools</p>
        <nav className="woo-nav">{navigation("branches", "Branches")}{navigation("tags", "Tags")}{navigation("stashes", "Stashes")}</nav>
        <nav className="woo-nav woo-nav-bottom">{navigation("merge", "Merge / Conflicts")}</nav>
      </aside>}
      <section className="workspace woo-workspace" data-view={activeView} data-detail-position={detailPosition} style={{ "--woo-master": masterSize } as CSSProperties}>{children}</section>
    </div>
    <footer className="woo-statusbar"><span>{repository ? `${repository.branch ?? "Detached HEAD"} · ${changeCount} changes` : "Open a Git repository to begin"}</span><span>{busy ?? "Ready"}</span></footer>
  </main>;
}
