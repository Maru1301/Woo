import { useEffect, useRef, useState, type CSSProperties } from "react";
import DiffViewer from "../diff/DiffViewer";
import GraphRowView, { GRAPH_ROW_HEIGHT, graphWidth } from "./GraphRowView";
import { FilePresentation, FileViewToggle, type FileViewMode } from "../changes/FilePresentation";
import { ContextMenu } from "../../components/ui/ContextMenu";
import { getCommitFiles, getCommitHistory, messageForError, type CommitHistoryPage, type CommitInfo, type FileChange, type GraphRow, type ResetMode } from "../../lib/repository";

const ROW_HEIGHT = GRAPH_ROW_HEIGHT;
const DEFAULT_VIEW_HEIGHT = 400;

export default function HistoryPanel({ refreshToken = 0, actionBusy = false, onAction, onTag, onBranch, currentHead, rebaseTargets = [], onRebase }: { refreshToken?: number; actionBusy?: boolean; onAction?: (action: "cherry_pick" | "revert" | "reset", hash: string, mode?: ResetMode) => Promise<void>; onTag?: (hash: string) => Promise<void>; onBranch?: (hash: string) => Promise<void>; currentHead?: string | null; rebaseTargets?: { name: string; hash: string }[]; onRebase?: (name: string) => Promise<void> }) {
  const [history, setHistory] = useState<{ commits: CommitInfo[]; graphRows: GraphRow[] }>({ commits: [], graphRows: [] });
  const [maxLanes, setMaxLanes] = useState(1);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selectedHash, setSelectedHash] = useState<string | null>(null);
  const [selectedSnapshot, setSelectedSnapshot] = useState<CommitInfo | null>(null);
  const [commitFiles, setCommitFiles] = useState<FileChange[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [filesError, setFilesError] = useState("");
  const [selectedFile, setSelectedFile] = useState<FileChange | null>(null);
  const [commitFilesMode, setCommitFilesMode] = useState<FileViewMode>("list");
  const [commitMenu, setCommitMenu] = useState<{ x: number; y: number; commit: CommitInfo } | null>(null);
  const [resetTarget, setResetTarget] = useState<string | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewHeight, setViewHeight] = useState(DEFAULT_VIEW_HEIGHT);
  const [reload, setReload] = useState(0);
  const [resetMode, setResetMode] = useState<ResetMode>("mixed");
  const [confirmHard, setConfirmHard] = useState(false);
  const [actionError, setActionError] = useState("");
  const [actionWorking, setActionWorking] = useState(false);
  const requestActive = useRef(false);
  const alive = useRef(false);
  const epoch = useRef(0);
  const nextCursor = useRef<string | null>(null);
  const more = useRef(true);
  const previousRefresh = useRef(refreshToken);
  const scrollElement = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = scrollElement.current;
    if (!element) return;
    const update = () => setViewHeight(element.clientHeight || DEFAULT_VIEW_HEIGHT);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [history.commits.length > 0]);

  async function loadPage(requestCursor: string | null) {
    if (requestActive.current || !more.current) return;
    requestActive.current = true;
    const requestEpoch = epoch.current;
    setLoading(true);
    setError("");
    try {
      const page: CommitHistoryPage = await getCommitHistory(requestCursor);
      if (!alive.current || epoch.current !== requestEpoch) return;
      if (page.commits.length !== page.graphRows.length) throw new Error("History and graph rows are out of sync. Reload history.");
      setHistory((current) => requestCursor === null
        ? { commits: page.commits, graphRows: page.graphRows }
        : { commits: [...current.commits, ...page.commits], graphRows: [...current.graphRows, ...page.graphRows] });
      if (requestCursor === null) setSelectedSnapshot((current) => current ? page.commits.find((commit) => commit.hash === current.hash) ?? current : null);
      setMaxLanes((current) => Math.max(requestCursor === null ? 1 : current, ...page.graphRows.map((row) => row.laneCount)));
      nextCursor.current = page.nextCursor;
      more.current = page.hasMore;
      setCursor(page.nextCursor);
      setHasMore(page.hasMore);
    } catch (cause) {
      if (alive.current && epoch.current === requestEpoch) setError(messageForError(cause));
    } finally {
      if (epoch.current === requestEpoch) {
        requestActive.current = false;
        if (alive.current) setLoading(false);
      }
    }
  }

  useEffect(() => {
    alive.current = true;
    epoch.current += 1;
    requestActive.current = false;
    more.current = true;
    nextCursor.current = null;
    setHistory({ commits: [], graphRows: [] });
    setMaxLanes(1);
    setCursor(null);
    setHasMore(true);
    setSelectedHash(null);
    setSelectedSnapshot(null);
    setCommitMenu(null);
    setConfirmHard(false); setActionError("");
    setScrollTop(0);
    void loadPage(null);
    return () => { alive.current = false; epoch.current += 1; };
  }, [reload]);

  // Fetch can move remote refs. Reload page one and graph topology, while
  // retaining the selected commit's metadata/diff until the user selects anew.
  useEffect(() => {
    if (previousRefresh.current === refreshToken) return;
    previousRefresh.current = refreshToken;
    epoch.current += 1;
    requestActive.current = false;
    more.current = true;
    nextCursor.current = null;
    setHistory({ commits: [], graphRows: [] });
    setMaxLanes(1);
    setCursor(null);
    setHasMore(true);
    setScrollTop(0);
    setCommitMenu(null);
    setSelectedSnapshot((current) => current ? { ...current, refs: [] } : null);
    if (scrollElement.current) scrollElement.current.scrollTop = 0;
    void loadPage(null);
  }, [refreshToken]);

  useEffect(() => {
    let cancelled = false;
    setCommitFiles([]);
    setSelectedFile(null);
    setFilesError("");
    if (!selectedHash) { setFilesLoading(false); return; }
    setFilesLoading(true);
    void getCommitFiles(selectedHash).then((files) => { if (!cancelled) setCommitFiles(files); })
      .catch((cause) => { if (!cancelled) setFilesError(messageForError(cause)); })
      .finally(() => { if (!cancelled) setFilesLoading(false); });
    return () => { cancelled = true; };
  }, [selectedHash]);

  const { commits, graphRows } = history;
  const graphColumnWidth = Math.max(96, graphWidth(maxLanes));
  const selected = commits.find((commit) => commit.hash === selectedHash) ?? (selectedSnapshot?.hash === selectedHash ? selectedSnapshot : null);
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 5);
  const end = Math.min(commits.length, start + Math.ceil(viewHeight / ROW_HEIGHT) + 10);

  function onScroll(element: HTMLDivElement) {
    setScrollTop(element.scrollTop);
    if (element.scrollTop + element.clientHeight >= element.scrollHeight - 250 && more.current && !requestActive.current && !error) {
      void loadPage(nextCursor.current);
    }
  }

  async function runAction(action: "cherry_pick" | "revert" | "reset", hash: string, mode?: ResetMode) {
    if (!onAction || actionBusy || actionWorking) return;
    setActionError(""); setActionWorking(true);
    try { await onAction(action, hash, mode); setConfirmHard(false); setResetTarget(null); }
    catch (cause) { if (alive.current) setActionError(messageForError(cause)); }
    finally { if (alive.current) setActionWorking(false); }
  }

  function selectCommit(commit: CommitInfo) {
    if (selectedHash === commit.hash) return;
    setSelectedHash(commit.hash);
    setSelectedSnapshot(commit);
    setCommitFiles([]);
    setSelectedFile(null);
    setFilesLoading(true);
    setConfirmHard(false);
    setActionError("");
  }

  return <section className="history-panel" aria-label="Commit history">
    <div className="woo-history-master">
    <div className="history-heading"><div><p className="eyebrow">HISTORY</p><h2>Commits</h2></div><button className="secondary" disabled={loading} onClick={() => setReload((value) => value + 1)}>Reload</button></div>
    {actionError && !resetTarget && <p className="error" role="alert">{actionError}</p>}
    <div className="woo-history-listhead" style={{ "--woo-graph-column": `${graphColumnWidth}px` } as CSSProperties}><span>Graph</span><span>Commit</span><span>Author</span><span>Date</span></div>
    {commits.length === 0 && loading && <p className="status-placeholder">Loading recent commits…</p>}
    {commits.length === 0 && !loading && !error && <p className="status-placeholder">No commits yet.</p>}
    {commits.length > 0 && <div ref={scrollElement} className="history-scroll" onScroll={(event) => onScroll(event.currentTarget)}>
      <div className="history-spacer" style={{ height: commits.length * ROW_HEIGHT }}>
        {commits.slice(start, end).map((commit, index) => <button type="button" className={`history-row ${selectedHash === commit.hash ? "selected" : ""}`} style={{ top: (start + index) * ROW_HEIGHT }} key={commit.hash} onClick={() => selectCommit(commit)} onContextMenu={(event) => { event.preventDefault(); selectCommit(commit); setCommitMenu({ x: event.clientX, y: event.clientY, commit }); }}>
          <GraphRowView row={graphRows[start + index]} width={graphColumnWidth} selected={selectedHash === commit.hash} />
          <span className="history-content"><span className="history-subject">{commit.subject || "(no subject)"}</span><span className="history-meta"><code>{commit.hash.slice(0, 10)}</code></span>
          {commit.refs.length > 0 && <span className="history-refs">{commit.refs.join(" · ")}</span>}</span>
          <span className="woo-history-author" title={commit.authorName}>{commit.authorName}</span><span className="woo-history-date" title={new Date(commit.timestamp).toLocaleString()}>{new Date(commit.timestamp).toLocaleDateString()}</span>
        </button>)}
      </div>
    </div>}
    <div className="history-paging">{loading && <span role="status">Loading older commits…</span>}{!loading && hasMore && commits.length > 0 && <button className="secondary" onClick={() => void loadPage(cursor)}>Load older</button>}{!hasMore && commits.length > 0 && <span>End of history</span>}</div>
    {error && <p className="error" role="alert">{error} <button className="secondary" onClick={() => setReload((value) => value + 1)}>Reload history</button></p>}
    </div>
    {selected && <div className="commit-details"><h3>Commit details</h3><dl>
      <div><dt>Subject</dt><dd>{selected.subject || "(no subject)"}</dd></div>
      <div><dt>Hash</dt><dd><code>{selected.hash}</code></dd></div>
      <div><dt>Parents</dt><dd>{selected.parentHashes.length ? selected.parentHashes.map((hash) => <code key={hash}>{hash}</code>) : "Root commit"}</dd></div>
      <div><dt>Author</dt><dd>{selected.authorName} &lt;{selected.authorEmail}&gt;</dd></div>
      <div><dt>Date</dt><dd>{selected.timestamp}</dd></div>
      <div><dt>Refs</dt><dd>{selected.refs.join(", ") || "None"}</dd></div>
    </dl>
      <div className="commit-files"><div className="woo-commit-files-heading"><h4>Changed files {filesLoading ? "(loading…)" : `(${commitFiles.length})`}</h4><FileViewToggle mode={commitFilesMode} onChange={setCommitFilesMode} /></div>
        {selected.parentHashes.length > 1 && <p className="commit-compare-note">Compared with the first parent.</p>}
        {selected.parentHashes.length === 0 && <p className="commit-compare-note">Initial commit, compared with an empty tree.</p>}
        {filesError && <p className="error" role="alert">{filesError}</p>}
        {!filesLoading && !filesError && commitFiles.length === 0 && <p className="status-placeholder">No file changes against {selected.parentHashes.length > 1 ? "the first parent" : "this commit's parent"}.</p>}
        {commitFiles.length > 0 && <FilePresentation files={commitFiles} mode={commitFilesMode} selectedPath={selectedFile?.path} onSelect={setSelectedFile} maxHeight={238} />}
      </div>
      {selectedFile && <DiffViewer key={`${selected.hash}:${selectedFile.path}:${selectedFile.oldPath ?? ""}`} source="commit" commit={selected.hash} change={selectedFile} />}
    </div>}
    {!selected && <div className="commit-details woo-history-empty"><p>Select a commit to inspect its files and diff.</p></div>}
    {commitMenu && <ContextMenu x={commitMenu.x} y={commitMenu.y} title={`${commitMenu.commit.hash.slice(0, 10)} · ${commitMenu.commit.subject}`} onClose={() => setCommitMenu(null)} actions={[
      { label: "Checkout commit", disabled: true, title: "Detached commit checkout is not available in Woo", onSelect: () => {} },
      { label: "Create branch here…", disabled: !onBranch || actionBusy || actionWorking || currentHead !== commitMenu.commit.hash, title: currentHead !== commitMenu.commit.hash ? "Branch creation currently targets HEAD" : undefined, onSelect: () => { if (onBranch) void onBranch(commitMenu.commit.hash).catch((cause) => setActionError(messageForError(cause))); } },
      { label: "Create tag here…", disabled: !onTag || actionBusy || actionWorking, onSelect: () => { if (onTag) void onTag(commitMenu.commit.hash).catch((cause) => setActionError(messageForError(cause))); } },
      { label: "Cherry-pick", disabled: !onAction || actionBusy || actionWorking || commitMenu.commit.parentHashes.length > 1, onSelect: () => void runAction("cherry_pick", commitMenu.commit.hash) },
      { label: "Revert", disabled: !onAction || actionBusy || actionWorking || commitMenu.commit.parentHashes.length > 1, onSelect: () => void runAction("revert", commitMenu.commit.hash) },
      { label: "Rebase current branch onto here…", disabled: !onRebase || actionBusy || actionWorking || !rebaseTargets.some((target) => target.hash === commitMenu.commit.hash), title: "Available when a different local branch points to this commit", onSelect: () => { const target = rebaseTargets.find((item) => item.hash === commitMenu.commit.hash); if (target && onRebase) void onRebase(target.name).catch((cause) => setActionError(messageForError(cause))); } },
      { label: "Reset current branch to here…", disabled: !onAction || actionBusy || actionWorking, danger: true, onSelect: () => { setResetTarget(commitMenu.commit.hash); setResetMode("mixed"); setConfirmHard(false); } },
      { label: "Copy commit hash", onSelect: () => void (navigator.clipboard?.writeText(commitMenu.commit.hash) ?? Promise.reject()).catch(() => setActionError("Could not copy the commit hash.")) },
    ]} />}
    {resetTarget && <div className="woo-dialog-backdrop" role="presentation"><div className="woo-reset-dialog" role="dialog" aria-modal="true" aria-label="Reset current branch">
      <h3>Reset current branch</h3><p>Move the current branch to <code>{resetTarget.slice(0, 10)}</code>.</p>
      <label htmlFor="reset-mode">Mode</label><select id="reset-mode" value={resetMode} disabled={actionBusy || actionWorking} onChange={(event) => { setResetMode(event.target.value as ResetMode); setConfirmHard(false); }}><option value="soft">Soft · keep index and working files</option><option value="mixed">Mixed · reset index, keep working files</option><option value="hard">Hard · discard tracked index and working changes</option></select>
      {resetMode === "hard" && <p className="error" role="alert">Hard reset discards tracked staged and working-tree changes. Untracked files are not cleaned; Woo refuses a reset that could overwrite them.</p>}
      {actionError && <p className="error" role="alert">{actionError}</p>}
      <div className="woo-dialog-actions"><button type="button" className="secondary" disabled={actionWorking} onClick={() => setResetTarget(null)}>Cancel</button><button type="button" disabled={actionBusy || actionWorking} onClick={() => { if (resetMode === "hard" && !confirmHard) { setConfirmHard(true); return; } void runAction("reset", resetTarget, resetMode); }}>{resetMode === "hard" && confirmHard ? "Confirm hard reset" : "Reset to commit"}</button></div>
    </div></div>}
  </section>;
}
