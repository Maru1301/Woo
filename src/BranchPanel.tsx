import { useState } from "react";
import type { BranchInfo, BranchList } from "./lib/repository";

export type BranchState =
  | { phase: "idle" | "loading"; data: null }
  | { phase: "ready"; data: BranchList }
  | { phase: "error"; data: null; message: string };

const ROW_HEIGHT = 42;

function BranchGroup({ title, branches, busy, onCheckout }: { title: string; branches: BranchInfo[]; busy: boolean; onCheckout?: (name: string) => void }) {
  const [scrollTop, setScrollTop] = useState(0);
  const height = Math.min(252, Math.max(42, branches.length * ROW_HEIGHT));
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 3);
  const end = Math.min(branches.length, start + Math.ceil(height / ROW_HEIGHT) + 6);
  return <div className="branch-group">
    <h3>{title}<span>{branches.length}</span></h3>
    {branches.length === 0 ? <p className="empty-group">No branches</p> : <div className="branch-scroll" style={{ height }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="branch-spacer" style={{ height: branches.length * ROW_HEIGHT }}>
        {branches.slice(start, end).map((branch, index) => <div className={`branch-row ${branch.isCurrent ? "current" : ""}`} style={{ top: (start + index) * ROW_HEIGHT }} key={branch.fullRefName}>
          <span className="branch-indicator" aria-label={branch.isCurrent ? "Current branch" : undefined}>{branch.isCurrent ? "●" : ""}</span>
          <span className="branch-name" title={branch.fullRefName}>{branch.name}</span>
          <code title={branch.targetHash}>{branch.targetHash.slice(0, 8)}</code>
          {branch.upstream && <span className="branch-upstream" title={`Upstream: ${branch.upstream}`}>↗ {branch.upstream}</span>}
          {onCheckout && !branch.isCurrent && <button className="secondary" disabled={busy} onClick={() => onCheckout(branch.name)}>Switch</button>}
        </div>)}
      </div>
    </div>}
  </div>;
}

export default function BranchPanel({ state, currentBranch, busy, onCreate, onCheckout, onRetry, error }: {
  state: BranchState;
  currentBranch: string | null;
  busy: boolean;
  onCreate: (name: string) => Promise<boolean>;
  onCheckout: (name: string) => void;
  onRetry: () => void;
  error: string;
}) {
  const [name, setName] = useState("");
  const branches = state.phase === "ready" ? state.data.branches : [];
  return <section className="branches-panel" aria-label="Branches">
    <div className="branches-heading"><div><p className="eyebrow">REFS</p><h2>Branches</h2></div><span className="branch-current">{currentBranch ? `Current: ${currentBranch}` : "Detached HEAD"}</span></div>
    <form className="branch-create" onSubmit={(event) => { event.preventDefault(); if (name.trim()) void onCreate(name.trim()).then((created) => { if (created) setName(""); }); }}>
      <label htmlFor="new-branch">Create local branch from HEAD</label>
      <div><input id="new-branch" value={name} onChange={(event) => setName(event.target.value)} placeholder="feature/name" disabled={busy} /><button disabled={busy || !name.trim()}>Create</button></div>
    </form>
    {state.phase === "loading" && <p className="status-placeholder">Loading branches…</p>}
    {state.phase === "error" && <p className="error" role="alert">{state.message} <button className="secondary" disabled={busy} onClick={onRetry}>Retry</button></p>}
    {error && <p className="error" role="alert">{error}</p>}
    {state.phase === "ready" && <div className="branch-groups">
      <BranchGroup title="Local" branches={branches.filter((branch) => branch.kind === "local")} busy={busy} onCheckout={onCheckout} />
      <BranchGroup title="Remote tracking" branches={branches.filter((branch) => branch.kind === "remote")} busy={busy} />
    </div>}
  </section>;
}
