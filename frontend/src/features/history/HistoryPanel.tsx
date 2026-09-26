import { useEffect, useRef, useState, type CSSProperties } from "react";
import DiffViewer from "../diff/DiffViewer";
import GraphRowView, { GRAPH_ROW_HEIGHT, graphWidth } from "./GraphRowView";
import { getCommitFiles, getCommitHistory, messageForError, type CommitHistoryPage, type CommitInfo, type FileChange, type GraphRow, type ResetMode } from "../../lib/repository";

const ROW_HEIGHT = GRAPH_ROW_HEIGHT;
const DEFAULT_VIEW_HEIGHT = 400;

export default function HistoryPanel({ refreshToken = 0, actionBusy = false, onAction }: { refreshToken?: number; actionBusy?: boolean; onAction?: (action: "cherry_pick" | "revert" | "reset", hash: string, mode?: ResetMode) => Promise<void> }) {
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
  const [fileScrollTop, setFileScrollTop] = useState(0);
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
    setSelectedSnapshot((current) => current ? { ...current, refs: [] } : null);
    if (scrollElement.current) scrollElement.current.scrollTop = 0;
    void loadPage(null);
  }, [refreshToken]);

  useEffect(() => {
    let cancelled = false;
    setCommitFiles([]);
    setSelectedFile(null);
    setFileScrollTop(0);
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
  const fileStart = Math.max(0, Math.floor(fileScrollTop / 34) - 4);
  const fileEnd = Math.min(commitFiles.length, fileStart + 16);

  function onScroll(element: HTMLDivElement) {
    setScrollTop(element.scrollTop);
    if (element.scrollTop + element.clientHeight >= element.scrollHeight - 250 && more.current && !requestActive.current && !error) {
      void loadPage(nextCursor.current);
    }
  }

  async function runAction(action: "cherry_pick" | "revert" | "reset", hash: string, mode?: ResetMode) {
    if (!onAction || actionBusy || actionWorking) return;
    setActionError(""); setActionWorking(true);
    try { await onAction(action, hash, mode); setConfirmHard(false); }
    catch (cause) { if (alive.current) setActionError(messageForError(cause)); }
    finally { if (alive.current) setActionWorking(false); }
  }

  return <section className="history-panel" aria-label="Commit history">
    <div className="woo-history-master">
    <div className="history-heading"><div><p className="eyebrow">HISTORY</p><h2>Commits</h2></div><button className="secondary" disabled={loading} onClick={() => setReload((value) => value + 1)}>Reload</button></div>
    <div className="woo-history-listhead" style={{ "--woo-graph-column": `${graphColumnWidth}px` } as CSSProperties}><span>Graph</span><span>Commit</span><span>Author</span><span>Date</span></div>
    {commits.length === 0 && loading && <p className="status-placeholder">Loading recent commits…</p>}
    {commits.length === 0 && !loading && !error && <p className="status-placeholder">No commits yet.</p>}
    {commits.length > 0 && <div ref={scrollElement} className="history-scroll" onScroll={(event) => onScroll(event.currentTarget)}>
      <div className="history-spacer" style={{ height: commits.length * ROW_HEIGHT }}>
        {commits.slice(start, end).map((commit, index) => <button type="button" className={`history-row ${selectedHash === commit.hash ? "selected" : ""}`} style={{ top: (start + index) * ROW_HEIGHT }} key={commit.hash} onClick={() => { if (selectedHash !== commit.hash) { setSelectedHash(commit.hash); setSelectedSnapshot(commit); setCommitFiles([]); setSelectedFile(null); setFilesLoading(true); setConfirmHard(false); setActionError(""); } }}>
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
      {onAction && <div className="history-actions"><h4>History actions</h4><p>Actions apply to the current branch. Cherry-pick and revert create commits; reset moves the branch to this commit.</p>
        <div className="conflict-side-actions"><button className="secondary" disabled={actionBusy || actionWorking || selected.parentHashes.length > 1} onClick={() => void runAction("cherry_pick", selected.hash)}>Cherry-pick</button><button className="secondary" disabled={actionBusy || actionWorking || selected.parentHashes.length > 1} onClick={() => void runAction("revert", selected.hash)}>Revert</button></div>
        {selected.parentHashes.length > 1 && <p className="management-hint">Merge commits need a mainline parent and cannot be cherry-picked or reverted here.</p>}
        <div className="conflict-side-actions"><label htmlFor="reset-mode">Reset mode</label><select id="reset-mode" value={resetMode} disabled={actionBusy || actionWorking} onChange={(event) => { setResetMode(event.target.value as ResetMode); setConfirmHard(false); }}><option value="soft">Soft · keep index and working files</option><option value="mixed">Mixed · reset index, keep working files</option><option value="hard">Hard · discard tracked index and working changes</option></select><button className="secondary" disabled={actionBusy || actionWorking} onClick={() => { if (resetMode === "hard" && !confirmHard) { setConfirmHard(true); return; } void runAction("reset", selected.hash, resetMode); }}>{resetMode === "hard" && confirmHard ? "Confirm hard reset" : "Reset to commit"}</button></div>
        {confirmHard && <p className="error" role="alert">Hard reset discards tracked staged and working-tree changes. Untracked files are not cleaned; Woo refuses a reset that could overwrite them. Click Confirm hard reset to proceed.</p>}
        {actionError && <p className="error" role="alert">{actionError}</p>}{actionWorking && <p role="status">Running history operation…</p>}
      </div>}
      <div className="commit-files"><h4>Changed files {filesLoading ? "(loading…)" : `(${commitFiles.length})`}</h4>
        {selected.parentHashes.length > 1 && <p className="commit-compare-note">Compared with the first parent.</p>}
        {selected.parentHashes.length === 0 && <p className="commit-compare-note">Initial commit, compared with an empty tree.</p>}
        {filesError && <p className="error" role="alert">{filesError}</p>}
        {!filesLoading && !filesError && commitFiles.length === 0 && <p className="status-placeholder">No file changes against {selected.parentHashes.length > 1 ? "the first parent" : "this commit's parent"}.</p>}
        {commitFiles.length > 0 && <div className="commit-file-scroll" style={{ height: Math.min(238, commitFiles.length * 34) }} onScroll={(event) => setFileScrollTop(event.currentTarget.scrollTop)}><div className="commit-file-spacer" style={{ height: commitFiles.length * 34 }}>
          {commitFiles.slice(fileStart, fileEnd).map((file, index) => <button className={`commit-file-row ${selectedFile?.path === file.path ? "selected" : ""}`} style={{ top: (fileStart + index) * 34 }} key={`${file.path}:${fileStart + index}`} onClick={() => setSelectedFile(file)}><span>{file.kind.replace("_", " ")}</span>{file.oldPath && `${file.oldPath} → `}{file.path}</button>)}
        </div></div>}
      </div>
      {selectedFile && <DiffViewer key={`${selected.hash}:${selectedFile.path}:${selectedFile.oldPath ?? ""}`} source="commit" commit={selected.hash} change={selectedFile} />}
    </div>}
    {!selected && <div className="commit-details woo-history-empty"><p>Select a commit to inspect its files and diff.</p></div>}
  </section>;
}
