import { useState } from "react";
import type { BranchInfo, BranchList } from "../../lib/repository";
import type { LoadState } from "../../app/repository-session/useRepositorySession";
import { ContextMenu } from "../../components/ui/ContextMenu";

export type BranchState = LoadState<BranchList>;

const ROW_HEIGHT = 42;

function BranchGroup({ title, branches, busy, onCheckout, onRename, onDelete }: { title: string; branches: BranchInfo[]; busy: boolean; onCheckout?: (name: string) => void; onRename?: (ref: string, name: string) => Promise<boolean>; onDelete?: (ref: string) => Promise<boolean> }) {
  const [scrollTop, setScrollTop] = useState(0);
  const [editing, setEditing] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [menu, setMenu] = useState<{ branch: BranchInfo; x: number; y: number } | null>(null);
  const height = Math.min(252, Math.max(42, branches.length * ROW_HEIGHT));
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 3);
  const end = Math.min(branches.length, start + Math.ceil(height / ROW_HEIGHT) + 6);
  return <div className="branch-group">
    <h3>{title}<span>{branches.length}</span></h3>
    {branches.length === 0 ? <p className="empty-group">No branches</p> : <div className="branch-scroll" style={{ height }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="branch-spacer" style={{ height: branches.length * ROW_HEIGHT }}>
        {branches.slice(start, end).map((branch, index) => <div className={`branch-row ${branch.isCurrent ? "current" : ""}`} style={{ top: (start + index) * ROW_HEIGHT }} key={branch.fullRefName} onContextMenu={(event) => { if (!onCheckout && !onRename && !onDelete) return; event.preventDefault(); setMenu({ branch, x: event.clientX, y: event.clientY }); }}>
          <span className="branch-indicator" aria-label={branch.isCurrent ? "Current branch" : undefined}>{branch.isCurrent ? "●" : ""}</span>
          {editing === branch.fullRefName ? <form className="branch-inline-form" onSubmit={(event) => { event.preventDefault(); if (newName.trim() && onRename) void onRename(branch.fullRefName, newName.trim()).then((ok) => { if (ok) setEditing(null); }); }}><input aria-label={`New name for ${branch.name}`} value={newName} onChange={(event) => setNewName(event.target.value)} disabled={busy} autoFocus /><button disabled={busy || !newName.trim()}>Save</button><button type="button" className="secondary" onClick={() => setEditing(null)}>Cancel</button></form> : onCheckout || onRename || onDelete ? <button type="button" className="branch-name woo-branch-menu-trigger" title={`${branch.fullRefName} · actions`} onClick={(event) => setMenu({ branch, x: event.clientX, y: event.clientY })}>{branch.name}</button> : <span className="branch-name" title={branch.fullRefName}>{branch.name}</span>}
          <code title={branch.targetHash}>{branch.targetHash.slice(0, 8)}</code>
          {branch.upstream && <span className="branch-upstream" title={`Upstream: ${branch.upstream}`}>↗ {branch.upstream}</span>}
        </div>)}
      </div>
    </div>}
    {menu && <ContextMenu x={menu.x} y={menu.y} title={menu.branch.name} onClose={() => setMenu(null)} actions={[
      { label: "Switch to branch", disabled: busy || menu.branch.isCurrent || !onCheckout, onSelect: () => onCheckout?.(menu.branch.name) },
      { label: "Rename branch…", disabled: busy || !onRename, onSelect: () => { setEditing(menu.branch.fullRefName); setNewName(menu.branch.name); } },
      { label: "Delete branch…", disabled: busy || menu.branch.isCurrent || !onDelete, danger: true, onSelect: () => { if (window.confirm(`Delete local branch ${menu.branch.name}? Git will refuse if it is unmerged. Repository files will not be deleted.`)) void onDelete?.(menu.branch.fullRefName); } },
    ]} />}
  </div>;
}

export default function BranchPanel({ state, currentBranch, busy, onCreate, onCheckout, onRename, onDelete, onRetry, error, compact = false }: {
  state: BranchState;
  currentBranch: string | null;
  busy: boolean;
  onCreate: (name: string) => Promise<boolean>;
  onCheckout: (name: string) => void;
  onRename: (ref: string, name: string) => Promise<boolean>;
  onDelete: (ref: string) => Promise<boolean>;
  onRetry: () => void;
  error: string;
  compact?: boolean;
}) {
  const [name, setName] = useState("");
  const branches = state.phase === "ready" ? state.data.branches : [];
  return <section className={`branches-panel ${compact ? "compact" : ""}`} aria-label="Branches">
    <div className="branches-heading"><div><p className="eyebrow">REFS</p><h2>Branches</h2></div><span className="branch-current">{currentBranch ? `Current: ${currentBranch}` : "Detached HEAD"}</span></div>
    {!compact && <form className="branch-create" onSubmit={(event) => { event.preventDefault(); if (name.trim()) void onCreate(name.trim()).then((created) => { if (created) setName(""); }); }}>
      <label htmlFor="new-branch">Create local branch from HEAD</label>
      <div><input id="new-branch" value={name} onChange={(event) => setName(event.target.value)} placeholder="feature/name" disabled={busy} /><button disabled={busy || !name.trim()}>Create</button></div>
    </form>}
    {state.phase === "loading" && <p className="status-placeholder">Loading branches…</p>}
    {state.phase === "error" && <p className="error" role="alert">{state.message} <button className="secondary" disabled={busy} onClick={onRetry}>Retry</button></p>}
    {error && <p className="error" role="alert">{error}</p>}
    {state.phase === "ready" && <div className="branch-groups">
      <BranchGroup title="Local" branches={branches.filter((branch) => branch.kind === "local")} busy={busy} onCheckout={onCheckout} onRename={compact ? undefined : onRename} onDelete={compact ? undefined : onDelete} />
      <BranchGroup title="Remote tracking" branches={branches.filter((branch) => branch.kind === "remote")} busy={busy} />
    </div>}
  </section>;
}
