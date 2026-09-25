import { useEffect, useRef, useState } from "react";
import DiffViewer from "./DiffViewer";
import GraphRowView, { GRAPH_ROW_HEIGHT, graphWidth } from "./GraphRowView";
import { getCommitFiles, getCommitHistory, messageForError, type CommitHistoryPage, type CommitInfo, type FileChange, type GraphRow } from "./lib/repository";

const ROW_HEIGHT = GRAPH_ROW_HEIGHT;
const VIEW_HEIGHT = 400;

export default function HistoryPanel({ refreshToken = 0 }: { refreshToken?: number }) {
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
  const [reload, setReload] = useState(0);
  const requestActive = useRef(false);
  const alive = useRef(false);
  const epoch = useRef(0);
  const nextCursor = useRef<string | null>(null);
  const more = useRef(true);
  const previousRefresh = useRef(refreshToken);
  const scrollElement = useRef<HTMLDivElement>(null);

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
  const selected = commits.find((commit) => commit.hash === selectedHash) ?? (selectedSnapshot?.hash === selectedHash ? selectedSnapshot : null);
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 5);
  const end = Math.min(commits.length, start + Math.ceil(VIEW_HEIGHT / ROW_HEIGHT) + 10);
  const fileStart = Math.max(0, Math.floor(fileScrollTop / 34) - 4);
  const fileEnd = Math.min(commitFiles.length, fileStart + 16);

  function onScroll(element: HTMLDivElement) {
    setScrollTop(element.scrollTop);
    if (element.scrollTop + element.clientHeight >= element.scrollHeight - 250 && more.current && !requestActive.current && !error) {
      void loadPage(nextCursor.current);
    }
  }

  return <section className="history-panel" aria-label="Commit history">
    <div className="history-heading"><div><p className="eyebrow">HISTORY</p><h2>Commits</h2></div><button className="secondary" disabled={loading} onClick={() => setReload((value) => value + 1)}>Reload</button></div>
    {commits.length === 0 && loading && <p className="status-placeholder">Loading recent commits…</p>}
    {commits.length === 0 && !loading && !error && <p className="status-placeholder">No commits yet.</p>}
    {commits.length > 0 && <div ref={scrollElement} className="history-scroll" style={{ height: VIEW_HEIGHT }} onScroll={(event) => onScroll(event.currentTarget)}>
      <div className="history-spacer" style={{ height: commits.length * ROW_HEIGHT }}>
        {commits.slice(start, end).map((commit, index) => <button type="button" className={`history-row ${selectedHash === commit.hash ? "selected" : ""}`} style={{ top: (start + index) * ROW_HEIGHT }} key={commit.hash} onClick={() => { if (selectedHash !== commit.hash) { setSelectedHash(commit.hash); setSelectedSnapshot(commit); setCommitFiles([]); setSelectedFile(null); setFilesLoading(true); } }}>
          <GraphRowView row={graphRows[start + index]} width={graphWidth(maxLanes)} selected={selectedHash === commit.hash} />
          <span className="history-content"><span className="history-subject">{commit.subject || "(no subject)"}</span><span className="history-meta"><code>{commit.hash.slice(0, 10)}</code> · {commit.authorName} · {new Date(commit.timestamp).toLocaleString()}</span>
          {commit.refs.length > 0 && <span className="history-refs">{commit.refs.join(" · ")}</span>}</span>
        </button>)}
      </div>
    </div>}
    <div className="history-paging">{loading && <span role="status">Loading older commits…</span>}{!loading && hasMore && commits.length > 0 && <button className="secondary" onClick={() => void loadPage(cursor)}>Load older</button>}{!hasMore && commits.length > 0 && <span>End of history</span>}</div>
    {error && <p className="error" role="alert">{error} <button className="secondary" onClick={() => setReload((value) => value + 1)}>Reload history</button></p>}
    {selected && <div className="commit-details"><h3>Commit details</h3><dl>
      <div><dt>Subject</dt><dd>{selected.subject || "(no subject)"}</dd></div>
      <div><dt>Hash</dt><dd><code>{selected.hash}</code></dd></div>
      <div><dt>Parents</dt><dd>{selected.parentHashes.length ? selected.parentHashes.map((hash) => <code key={hash}>{hash}</code>) : "Root commit"}</dd></div>
      <div><dt>Author</dt><dd>{selected.authorName} &lt;{selected.authorEmail}&gt;</dd></div>
      <div><dt>Date</dt><dd>{selected.timestamp}</dd></div>
      <div><dt>Refs</dt><dd>{selected.refs.join(", ") || "None"}</dd></div>
    </dl>
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
  </section>;
}
