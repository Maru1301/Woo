import { useEffect, useMemo, useState } from "react";
import { getCommitDiff, getStagedDiff, getUnstagedDiff, getUntrackedDiff, messageForError, type DiffFile, type DiffLine, type FileChange } from "./lib/repository";

export type DiffSource = "unstaged" | "staged" | "untracked" | "commit";
const ROW_HEIGHT = 26;
const VIEW_HEIGHT = 420;

export default function DiffViewer({ source, change, commit }: { source: DiffSource; change: FileChange; commit?: string }) {
  const [diff, setDiff] = useState<DiffFile | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setDiff(null);
    setError("");
    setLoading(true);
    setScrollTop(0);
    const request = source === "commit" ? getCommitDiff(commit!, change)
      : source === "staged" ? getStagedDiff(change)
      : source === "untracked" ? getUntrackedDiff(change)
      : getUnstagedDiff(change);
    void request.then((result) => { if (!cancelled) setDiff(result); })
      .catch((cause) => { if (!cancelled) setError(messageForError(cause)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [source, commit, change.path, change.oldPath, change.kind]);

  const rows = useMemo(() => diff?.hunks.flatMap((hunk) => [{ header: hunk.header, line: null as DiffLine | null }, ...hunk.lines.map((line) => ({ header: null as string | null, line }))]) ?? [], [diff]);
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 5);
  const end = Math.min(rows.length, start + Math.ceil(VIEW_HEIGHT / ROW_HEIGHT) + 10);
  return <section className="diff-panel" aria-label="File diff">
    <div className="diff-heading"><div><p className="eyebrow">{source === "commit" ? "COMMIT DIFF" : source.toUpperCase()}</p><h3>{change.path}</h3>{change.oldPath && <p>From {change.oldPath}</p>}</div><span>{change.kind.replace("_", " ")}</span></div>
    {loading && <p className="status-placeholder" role="status">Loading diff…</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {diff?.isBinary && <p className="status-placeholder">Binary file; text diff is unavailable.</p>}
    {diff && !diff.isBinary && rows.length === 0 && <p className="status-placeholder">No textual changes in this file.</p>}
    {diff && !diff.isBinary && rows.length > 0 && <div className="diff-scroll" style={{ height: Math.min(VIEW_HEIGHT, rows.length * ROW_HEIGHT) }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="diff-spacer" style={{ height: rows.length * ROW_HEIGHT }}>
        {rows.slice(start, end).map((row, index) => <div className={`diff-row ${row.header ? "hunk" : `line-${row.line!.kind}`}`} style={{ top: (start + index) * ROW_HEIGHT }} key={start + index}>
          {row.header ? <span className="diff-hunk-label">{row.header}</span> : <><span className="diff-number">{row.line!.oldLineNumber ?? ""}</span><span className="diff-number">{row.line!.newLineNumber ?? ""}</span><span className="diff-sign">{row.line!.kind === "addition" ? "+" : row.line!.kind === "deletion" ? "−" : row.line!.kind === "no_newline" ? "↳" : " "}</span><span className="diff-content">{row.line!.kind === "no_newline" ? "No newline at end of file" : row.line!.content}</span></>}
        </div>)}
      </div>
    </div>}
  </section>;
}
