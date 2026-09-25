import { useEffect, useMemo, useRef, useState } from "react";
import { getCommitDiff, getStagedDiff, getUnstagedDiff, getUntrackedDiff, messageForError, type DiffFile, type DiffLine, type FileChange, type PartialSelection, type PartialStageResult } from "./lib/repository";

export type DiffSource = "unstaged" | "staged" | "untracked" | "commit";
const ROW_HEIGHT = 26;
const VIEW_HEIGHT = 420;

export default function DiffViewer({ source, change, commit, busy = false, onPartial }: { source: DiffSource; change: FileChange; commit?: string; busy?: boolean; onPartial?: (selection: PartialSelection) => Promise<PartialStageResult> }) {
  const [diff, setDiff] = useState<DiffFile | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [scrollTop, setScrollTop] = useState(0);
  const [selected, setSelected] = useState<{ hunk: number; lines: Set<number> } | null>(null);
  const [mutating, setMutating] = useState(false);
  const requestEpoch = useRef(0);

  useEffect(() => {
    let cancelled = false;
    const epoch = ++requestEpoch.current;
    setDiff(null); setError(""); setLoading(true); setScrollTop(0); setSelected(null);
    const request = source === "commit" ? getCommitDiff(commit!, change)
      : source === "staged" ? getStagedDiff(change)
      : source === "untracked" ? getUntrackedDiff(change)
      : getUnstagedDiff(change);
    void request.then((result) => { if (!cancelled && epoch === requestEpoch.current) setDiff(result); })
      .catch((cause) => { if (!cancelled && epoch === requestEpoch.current) setError(messageForError(cause)); })
      .finally(() => { if (!cancelled && epoch === requestEpoch.current) setLoading(false); });
    return () => { cancelled = true; };
  }, [source, commit, change.path, change.oldPath, change.kind]);

  const rows = useMemo(() => diff?.hunks.flatMap((hunk, hunkIndex) => [{ header: hunk.header, line: null as DiffLine | null, hunkIndex, lineIndex: -1 }, ...hunk.lines.map((line, lineIndex) => ({ header: null as string | null, line, hunkIndex, lineIndex }))]) ?? [], [diff]);
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 5);
  const end = Math.min(rows.length, start + Math.ceil(VIEW_HEIGHT / ROW_HEIGHT) + 10);
  const canPartial = !!onPartial && (source === "staged" || source === "unstaged") && !!diff?.partialStageable;

  async function apply(hunkIndex: number, lineIndices: number[] | null) {
    if (!canPartial || !diff || busy || mutating) return;
    setMutating(true); setError(""); requestEpoch.current += 1;
    try {
      const result = await onPartial!({ revision: diff.revision, hunkIndex, lineIndices });
      setDiff(source === "staged" ? result.stagedDiff : result.unstagedDiff);
      setSelected(null);
      if (result.error) setError(result.error.message);
    } catch (cause) {
      setDiff(null); setSelected(null);
      setError(`${messageForError(cause)} Refresh the file diff before trying again.`);
    } finally { setMutating(false); }
  }

  function toggle(hunk: number, line: number) {
    setSelected((current) => {
      const lines = current?.hunk === hunk ? new Set(current.lines) : new Set<number>();
      if (lines.has(line)) lines.delete(line); else lines.add(line);
      return lines.size ? { hunk, lines } : null;
    });
  }

  return <section className="diff-panel" aria-label="File diff">
    <div className="diff-heading"><div><p className="eyebrow">{source === "commit" ? "COMMIT DIFF" : source.toUpperCase()}</p><h3>{change.path}</h3>{change.oldPath && <p>From {change.oldPath}</p>}</div><span>{change.kind.replace("_", " ")}</span></div>
    {loading && <p className="status-placeholder" role="status">Loading diff…</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {diff?.isBinary && <p className="status-placeholder">Binary file; text diff is unavailable.</p>}
    {diff && !canPartial && (source === "staged" || source === "unstaged") && <p className="status-placeholder">Use the whole-file action for this file type.</p>}
    {canPartial && selected && <div className="partial-actions"><span>{selected.lines.size} changed line(s) selected in one hunk</span><button disabled={busy || mutating} onClick={() => void apply(selected.hunk, [...selected.lines].sort((a, b) => a - b))}>{source === "staged" ? "Unstage" : "Stage"} Selected Lines</button><button className="secondary" onClick={() => setSelected(null)}>Clear</button></div>}
    {diff && !diff.isBinary && rows.length === 0 && <p className="status-placeholder">No textual changes in this file.</p>}
    {diff && !diff.isBinary && rows.length > 0 && <div className="diff-scroll" style={{ height: Math.min(VIEW_HEIGHT, rows.length * ROW_HEIGHT) }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="diff-spacer" style={{ height: rows.length * ROW_HEIGHT }}>
        {rows.slice(start, end).map((row, index) => <div className={`diff-row ${row.header ? "hunk" : `line-${row.line!.kind}`}`} style={{ top: (start + index) * ROW_HEIGHT }} key={start + index}>
          {row.header ? <><span className="diff-hunk-label">{row.header}</span>{canPartial && <button className="diff-hunk-action" disabled={busy || mutating} onClick={() => void apply(row.hunkIndex, null)}>{source === "staged" ? "Unstage Hunk" : "Stage Hunk"}</button>}</> : <><span className="diff-select">{canPartial && (row.line!.kind === "addition" || row.line!.kind === "deletion") && <input type="checkbox" aria-label={`Select ${row.line!.kind} line ${row.line!.newLineNumber ?? row.line!.oldLineNumber}`} checked={selected?.hunk === row.hunkIndex && selected.lines.has(row.lineIndex)} disabled={busy || mutating} onChange={() => toggle(row.hunkIndex, row.lineIndex)} />}</span><span className="diff-number">{row.line!.oldLineNumber ?? ""}</span><span className="diff-number">{row.line!.newLineNumber ?? ""}</span><span className="diff-sign">{row.line!.kind === "addition" ? "+" : row.line!.kind === "deletion" ? "−" : row.line!.kind === "no_newline" ? "↵" : " "}</span><span className="diff-content">{row.line!.kind === "no_newline" ? "No newline at end of file" : row.line!.content}</span></>}
        </div>)}
      </div>
    </div>}
  </section>;
}
