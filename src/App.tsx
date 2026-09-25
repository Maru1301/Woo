import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import HistoryPanel from "./HistoryPanel";
import RemotePanel from "./RemotePanel";
import BranchPanel, { type BranchState } from "./BranchPanel";
import DiffViewer, { type DiffSource } from "./DiffViewer";
import {
  checkoutBranch, commitStaged, createBranch, errorCode, getBranches, getRepositoryStatus, messageForError, openRepository, stageAll, stageFile,
  unstageAll, unstageFile, type FileChange, type RemoteRefresh, type RepositoryInfo, type RepositoryStatus,
} from "./lib/repository";

type StatusState =
  | { phase: "idle" | "loading"; data: null }
  | { phase: "ready"; data: RepositoryStatus }
  | { phase: "error"; data: null; message: string };

const ROW_HEIGHT = 44;

function FileGroup({ title, files, action, onAction, onSelect, busy }: {
  title: string;
  files: FileChange[];
  action?: string;
  onAction?: (change: FileChange) => void;
  onSelect?: (change: FileChange) => void;
  busy: boolean;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const height = Math.min(280, Math.max(44, files.length * ROW_HEIGHT));
  const start = Math.min(
    Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 4),
    Math.max(0, files.length - Math.ceil(height / ROW_HEIGHT)),
  );
  const end = Math.min(files.length, start + Math.ceil(height / ROW_HEIGHT) + 8);
  return <section className="file-group" aria-label={title}>
    <h3>{title}<span>{files.length}</span></h3>
    {files.length === 0 ? <p className="empty-group">No files</p> : <div className="file-scroll" style={{ height }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="file-spacer" style={{ height: files.length * ROW_HEIGHT }}>
        {files.slice(start, end).map((file, index) => <div className="file-row" style={{ top: (start + index) * ROW_HEIGHT }} key={`${file.path}:${index}`}>
          <span className={`change-kind kind-${file.kind}`}>{file.kind.replace("_", " ")}</span>
          {onSelect ? <button className="file-select file-path" disabled={busy} title={`View diff for ${file.path}`} onClick={() => onSelect(file)}>{file.oldPath && <span className="old-path">{file.oldPath} → </span>}{file.path}</button> : <span className="file-path" title={file.path}>{file.path}</span>}
          {action && onAction && <button className="row-action" disabled={busy} onClick={() => onAction(file)}>{action}</button>}
        </div>)}
      </div>
    </div>}
  </section>;
}

export default function App() {
  const [path, setPath] = useState("");
  const [repository, setRepository] = useState<RepositoryInfo | null>(null);
  const [openError, setOpenError] = useState("");
  const [status, setStatus] = useState<StatusState>({ phase: "idle", data: null });
  const [busy, setBusy] = useState<string | null>(null);
  const [commitMessage, setCommitMessage] = useState("");
  const [commitError, setCommitError] = useState("");
  const [commitNotice, setCommitNotice] = useState("");
  const [selectedChange, setSelectedChange] = useState<{ source: DiffSource; change: FileChange } | null>(null);
  const [branches, setBranches] = useState<BranchState>({ phase: "idle", data: null });
  const [branchError, setBranchError] = useState("");
  const [historyVersion, setHistoryVersion] = useState(0);
  const [historyRefreshVersion, setHistoryRefreshVersion] = useState(0);
  const [remoteBusy, setRemoteBusy] = useState(false);
  const [sessionVersion, setSessionVersion] = useState(0);
  const branchRequest = useRef(0);
  const busyRef = useRef(false);

  async function loadBranches() {
    const request = ++branchRequest.current;
    setBranchError("");
    setBranches({ phase: "loading", data: null });
    try {
      const data = await getBranches();
      if (request === branchRequest.current) setBranches({ phase: "ready", data });
    } catch (cause) {
      if (request === branchRequest.current) setBranches({ phase: "error", data: null, message: messageForError(cause) });
    }
  }

  async function loadRepository(selectedPath: string) {
    if (!selectedPath.trim() || busyRef.current) return;
    busyRef.current = true;
    setBusy("Opening repository");
    setOpenError("");
    setRepository(null);
    setRemoteBusy(false);
    branchRequest.current += 1;
    setBranches({ phase: "idle", data: null });
    setBranchError("");
    setStatus({ phase: "idle", data: null });
    setSelectedChange(null);
    setCommitMessage("");
    setCommitError("");
    setCommitNotice("");
    try {
      const info = await openRepository(selectedPath.trim());
      setRepository(info);
      setSessionVersion((value) => value + 1);
      setHistoryVersion((value) => value + 1);
      void loadBranches();
      setPath(info.path);
      setStatus({ phase: "loading", data: null });
      try {
        setStatus({ phase: "ready", data: await getRepositoryStatus() });
      } catch (cause) {
        setStatus({ phase: "error", data: null, message: messageForError(cause) });
      }
    } catch (cause) {
      setOpenError(messageForError(cause));
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function chooseRepository() {
    if (busyRef.current) return;
    try {
      const selected = await open({ directory: true, multiple: false, title: "Open Git repository" });
      if (selected) {
        setPath(selected);
        await loadRepository(selected);
      }
    } catch (cause) {
      setOpenError(messageForError(cause));
    }
  }

  async function refresh() {
    if (busyRef.current || remoteBusy || !repository) return;
    busyRef.current = true;
    setBusy("Refreshing changes");
    setStatus({ phase: "loading", data: null });
    setSelectedChange(null);
    try {
      setStatus({ phase: "ready", data: await getRepositoryStatus() });
    } catch (cause) {
      setStatus({ phase: "error", data: null, message: messageForError(cause) });
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function changeIndex(label: string, operation: () => Promise<RepositoryStatus>) {
    if (busyRef.current || remoteBusy || !repository) return;
    busyRef.current = true;
    setBusy(label);
    setSelectedChange(null);
    setCommitNotice("");
    try {
      setStatus({ phase: "ready", data: await operation() });
      setCommitError("");
    } catch (cause) {
      setStatus({ phase: "error", data: null, message: messageForError(cause) });
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function commit() {
    if (busyRef.current || remoteBusy || !repository) return;
    if (!commitMessage.trim()) {
      setCommitError("Enter a commit message.");
      return;
    }
    if (status.phase !== "ready") {
      setCommitError("Refresh changes before committing.");
      return;
    }
    if (status.data.staged.length === 0) {
      setCommitError("Stage at least one change before committing.");
      return;
    }
    busyRef.current = true;
    setBusy("Committing staged changes");
    setSelectedChange(null);
    setCommitError("");
    setCommitNotice("");
    try {
      const result = await commitStaged(commitMessage);
      setRepository((current) => current ? { ...current, head: result.head } : current);
      branchRequest.current += 1;
      if (branches.phase === "ready") {
        setBranches({ phase: "ready", data: { branches: branches.data.branches.map((branch) => branch.isCurrent ? { ...branch, targetHash: result.head.hash } : branch) } });
      } else {
        void loadBranches();
      }
      setHistoryVersion((value) => value + 1);
      setStatus({ phase: "ready", data: result.status });
      setCommitMessage("");
      setCommitNotice(`Committed ${result.head.hash.slice(0, 10)}.`);
    } catch (cause) {
      const message = messageForError(cause);
      setCommitError(message);
      setStatus({ phase: "error", data: null, message: "Changes may have changed. Refresh to continue." });
      if (errorCode(cause) === "commit_refresh_failed") {
        setRepository(null);
        setOpenError(message);
      }
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function createLocalBranch(name: string): Promise<boolean> {
    if (busyRef.current || remoteBusy || !repository) return false;
    busyRef.current = true;
    setBusy(`Creating ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      setBranches({ phase: "ready", data: await createBranch(name) });
      return true;
    } catch (cause) {
      setBranchError(messageForError(cause));
      if (errorCode(cause) === "branch_refresh_failed") setBranches({ phase: "error", data: null, message: "Branch list is out of date. Retry to refresh." });
      return false;
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function switchBranch(name: string) {
    if (busyRef.current || remoteBusy || !repository) return;
    busyRef.current = true;
    setBusy(`Switching to ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const result = await checkoutBranch(name);
      setRepository((current) => current ? { ...current, branch: result.branch, head: result.head } : current);
      setBranches({ phase: "ready", data: result.branches });
      setStatus({ phase: "ready", data: result.status });
      setSelectedChange(null);
      setHistoryVersion((value) => value + 1);
      setCommitNotice("");
      setCommitError("");
    } catch (cause) {
      const message = messageForError(cause);
      setBranchError(message);
      if (errorCode(cause) === "checkout_refresh_failed") {
        setRepository(null);
        setStatus({ phase: "idle", data: null });
        setBranches({ phase: "idle", data: null });
        setSelectedChange(null);
        setOpenError("Branch switched, but its updated state could not be loaded. Reopen the repository.");
      }
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  function applyRemoteRefresh(result: RemoteRefresh) {
    branchRequest.current += 1;
    setBranches({ phase: "ready", data: result.branches });
    if (result.head) {
      const currentBranch = result.branches.branches.find((branch) => branch.isCurrent)?.name ?? null;
      setRepository((current) => current ? { ...current, branch: currentBranch, head: result.head } : current);
    }
    if (result.status) setStatus({ phase: "ready", data: result.status });
    if (result.clearDiff) setSelectedChange(null);
    if (result.resetHistory) setHistoryVersion((value) => value + 1);
    else if (result.refreshHistory) setHistoryRefreshVersion((value) => value + 1);
  }

  const changes = status.phase === "ready" ? status.data : null;
  const hasStageable = !!changes && changes.unstaged.length + changes.untracked.length + changes.conflicted.length > 0;
  const hasStaged = !!changes && changes.staged.length > 0;
  const controlsBusy = !!busy || remoteBusy;

  return <main className="app-shell">
    <header className="topbar"><div className="brand"><span className="brand-mark">W</span><span>Woo</span></div><span className="milestone">Remotes</span></header>
    <section className="workspace">
      <div className="intro"><p className="eyebrow">YOUR WORKSPACE</p><h1>Open a repository</h1><p>Inspect changes and prepare files for your next commit.</p></div>
      <div className="open-panel">
        <label htmlFor="repo-path">Repository path</label>
        <div className="path-controls">
          <input id="repo-path" value={path} onChange={(event) => setPath(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void loadRepository(path); }} placeholder="C:\\path\\to\\repository" disabled={!!busy} />
          <button className="secondary" onClick={() => void chooseRepository()} disabled={!!busy}>Browse…</button>
          <button onClick={() => void loadRepository(path)} disabled={!!busy || !path.trim()}>{busy === "Opening repository" ? "Opening…" : "Open"}</button>
        </div>
        {openError && <p className="error" role="alert">{openError}</p>}
      </div>
      {repository && <>
        <section className="repo-card" aria-label="Repository information">
          <div className="repo-heading"><span className="repo-icon">⌘</span><div><p className="eyebrow">REPOSITORY</p><h2>{repository.path.split(/[\\/]/).filter(Boolean).at(-1)}</h2></div></div>
          <dl>
            <div><dt>Path</dt><dd className="path-value">{repository.path}</dd></div>
            <div><dt>Current branch</dt><dd>{repository.branch ?? "Detached HEAD"}</dd></div>
            <div><dt>HEAD</dt><dd>{repository.head ? <><code>{repository.head.hash.slice(0, 10)}</code><span className="subject">{repository.head.subject}</span></> : "No commits yet"}</dd></div>
          </dl>
        </section>
        <RemotePanel key={`${repository.path}:${sessionVersion}`} onComplete={applyRemoteRefresh} onInconsistent={(message) => { setRepository(null); setStatus({ phase: "idle", data: null }); setOpenError(`${message} Reopen the repository.`); }} onBusyChange={setRemoteBusy} localBusy={!!busy} />
        <BranchPanel state={branches} currentBranch={repository.branch} busy={controlsBusy} onCreate={createLocalBranch} onCheckout={(name) => void switchBranch(name)} onRetry={() => void loadBranches()} error={branchError} />
        <section className="changes-panel" aria-label="Working tree changes">
          <div className="changes-heading"><div><p className="eyebrow">WORKING TREE</p><h2>Changes</h2></div><div className="change-actions">
            <button className="secondary" disabled={controlsBusy} onClick={() => void refresh()}>Refresh</button>
            <button className="secondary" disabled={controlsBusy || !hasStaged} onClick={() => void changeIndex("Unstaging all", unstageAll)}>Unstage All</button>
            <button disabled={controlsBusy || !hasStageable} onClick={() => void changeIndex("Staging all", stageAll)}>Stage All</button>
          </div></div>
          {busy && <p className="operation-feedback" role="status">{busy}…</p>}
          {status.phase === "loading" && <p className="status-placeholder">Loading changes…</p>}
          {status.phase === "error" && <p className="error" role="alert">{status.message}</p>}
          {changes && <div className="change-groups">
            <FileGroup title="Staged" files={changes.staged} action="Unstage" onSelect={(file) => setSelectedChange({ source: "staged", change: file })} onAction={(file) => void changeIndex(`Unstaging ${file.path}`, () => unstageFile(file))} busy={controlsBusy} />
            <FileGroup title="Unstaged" files={changes.unstaged} action="Stage" onSelect={(file) => setSelectedChange({ source: "unstaged", change: file })} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(file))} busy={controlsBusy} />
            <FileGroup title="Untracked" files={changes.untracked} action="Stage" onSelect={(file) => setSelectedChange({ source: "untracked", change: file })} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(file))} busy={controlsBusy} />
            {changes.conflicted.length > 0 && <FileGroup title="Conflicted" files={changes.conflicted} action="Stage" onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(file))} busy={controlsBusy} />}
          </div>}
          {selectedChange && changes && <DiffViewer key={`${selectedChange.source}:${selectedChange.change.path}:${selectedChange.change.oldPath ?? ""}`} source={selectedChange.source} change={selectedChange.change} />}
          <div className="commit-area">
            <div><h3>Commit staged changes</h3><p>Only files in Staged are included in this commit.</p></div>
            <label htmlFor="commit-message">Commit message</label>
            <textarea id="commit-message" value={commitMessage} disabled={controlsBusy} rows={4} placeholder="Describe your change" onChange={(event) => { setCommitMessage(event.target.value); setCommitError(""); setCommitNotice(""); }} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void commit(); } }} />
            <div className="commit-footer"><span>{!hasStaged ? "Stage a change to enable commit." : "Ctrl+Enter to commit"}</span><button disabled={controlsBusy || !hasStaged || status.phase !== "ready"} onClick={() => void commit()}>{busy === "Committing staged changes" ? "Committing…" : "Commit"}</button></div>
            {commitError && <p className="error" role="alert">{commitError}</p>}
            {commitNotice && <p className="commit-notice" role="status">{commitNotice}</p>}
          </div>
        </section>
        <HistoryPanel key={`${repository.path}:${historyVersion}`} refreshToken={historyRefreshVersion} />
      </>}
    </section>
  </main>;
}
