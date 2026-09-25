import { useState, type CSSProperties, type ReactNode } from "react";
import type { RepositoryInfo } from "../lib/repository";

export type WorkspaceView = "history" | "changes" | "manage";
type DetailPosition = "right" | "bottom";
type SizePreset = "balanced" | "history" | "detail";

export function WorkspaceLayout({ repository, activeView, onViewChange, onOpen, openDisabled, remoteControls, sidebar, changeCount, busy, children }: {
  repository: RepositoryInfo | null;
  activeView: WorkspaceView;
  onViewChange: (view: WorkspaceView) => void;
  onOpen: () => void;
  openDisabled?: boolean;
  remoteControls?: ReactNode;
  sidebar?: ReactNode;
  changeCount: number;
  busy: string | null;
  children: ReactNode;
}) {
  const [detailPosition, setDetailPosition] = useState<DetailPosition>("right");
  const [sizePreset, setSizePreset] = useState<SizePreset>("balanced");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const masterSize = detailPosition === "right"
    ? { balanced: "58%", history: "72%", detail: "38%" }[sizePreset]
    : { balanced: "43%", history: "60%", detail: "27%" }[sizePreset];
  const repoName = repository?.path.split(/[\\/]/).filter(Boolean).at(-1) ?? "Repository";

  return <main className="app-shell woo-shell">
    <header className="topbar woo-topbar">
      <div className="brand"><span className="brand-mark">W</span><span>Woo</span></div>
      <button className="woo-open-button secondary" onClick={onOpen} disabled={openDisabled} title="Open a repository">Open repository</button>
      <div className="woo-toolbar-spacer" />
      {remoteControls}
      {repository && <div className="woo-settings">
        <button className="secondary woo-settings-button" aria-label="Layout settings" aria-expanded={settingsOpen} onClick={() => setSettingsOpen((value) => !value)}>⚙</button>
        {settingsOpen && <div className="woo-settings-menu">
          <p>COMMIT DETAIL POSITION</p>
          <button className={detailPosition === "right" ? "active" : ""} onClick={() => { setDetailPosition("right"); setSettingsOpen(false); }}>Right</button>
          <button className={detailPosition === "bottom" ? "active" : ""} onClick={() => { setDetailPosition("bottom"); setSettingsOpen(false); }}>Bottom</button>
          <p>SIZE PRESET</p>
          <button className={sizePreset === "balanced" ? "active" : ""} onClick={() => { setSizePreset("balanced"); setSettingsOpen(false); }}>Balanced</button>
          <button className={sizePreset === "history" ? "active" : ""} onClick={() => { setSizePreset("history"); setSettingsOpen(false); }}>History focused</button>
          <button className={sizePreset === "detail" ? "active" : ""} onClick={() => { setSizePreset("detail"); setSettingsOpen(false); }}>Detail focused</button>
        </div>}
      </div>}
    </header>
    <div className="woo-tabs" aria-label="Open repository">
      {repository ? <span className="woo-repo-tab" title={repository.path}>{changeCount > 0 && <span className="woo-dirty">●</span>}{repoName}</span> : <span className="woo-repo-tab woo-repo-tab-empty">No repository open</span>}
    </div>
    <div className="woo-main">
      {repository && <aside className="woo-sidebar" aria-label="Workspace navigation">
        <nav className="woo-nav">
          <button className={activeView === "changes" ? "active" : ""} aria-current={activeView === "changes" ? "page" : undefined} onClick={() => onViewChange("changes")}>≡ <span>Changes</span><span className="woo-count">{changeCount}</span></button>
          <button className={activeView === "history" ? "active" : ""} aria-current={activeView === "history" ? "page" : undefined} onClick={() => onViewChange("history")}>◫ <span>History</span></button>
          <button className={activeView === "manage" ? "active" : ""} aria-current={activeView === "manage" ? "page" : undefined} onClick={() => onViewChange("manage")}>⌘ <span>Operations</span></button>
        </nav>
        {sidebar}
      </aside>}
      <section className="workspace woo-workspace" data-view={activeView} data-detail-position={detailPosition} style={{ "--woo-master": masterSize } as CSSProperties}>{children}</section>
    </div>
    <footer className="woo-statusbar"><span>{repository ? `${repository.branch ?? "Detached HEAD"} · ${changeCount} changes` : "Open a Git repository to begin"}</span><span>{busy ?? "Ready"}</span></footer>
  </main>;
}
