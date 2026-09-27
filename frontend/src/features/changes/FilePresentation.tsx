import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import type { FileChange } from "../../lib/repository";

export type FileViewMode = "list" | "tree";
type Row = { key: string; depth: number; directory?: string; file?: FileChange };

function rowsFor(files: FileChange[], mode: FileViewMode): Row[] {
  if (mode === "list") return files.map((file) => ({ key: file.path, depth: 0, file }));
  const rows: Row[] = [];
  const directories = new Set<string>();
  for (const file of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    const parts = file.path.split("/");
    for (let index = 1; index < parts.length; index++) {
      const path = parts.slice(0, index).join("/");
      if (!directories.has(path)) {
        directories.add(path);
        rows.push({ key: `dir:${path}`, depth: index - 1, directory: parts[index - 1] });
      }
    }
    rows.push({ key: `file:${file.path}`, depth: parts.length - 1, file });
  }
  return rows;
}

export function FileViewToggle({ mode, onChange }: { mode: FileViewMode; onChange: (mode: FileViewMode) => void }) {
  return <div className="woo-file-view-toggle" role="group" aria-label="File presentation">
    <button type="button" className={mode === "list" ? "active" : ""} aria-pressed={mode === "list"} onClick={() => onChange("list")}>List</button>
    <button type="button" className={mode === "tree" ? "active" : ""} aria-pressed={mode === "tree"} onClick={() => onChange("tree")}>Tree</button>
  </div>;
}

export function FilePresentation({ files, mode, selectedPath, busy = false, action, onAction, onSelect, onContextMenu, maxHeight = 280 }: {
  files: FileChange[];
  mode: FileViewMode;
  selectedPath?: string | null;
  busy?: boolean;
  action?: string;
  onAction?: (file: FileChange) => void;
  onSelect?: (file: FileChange) => void;
  onContextMenu?: (file: FileChange, event: MouseEvent) => void;
  maxHeight?: number;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const scrollElement = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => rowsFor(files, mode), [files, mode]);
  const rowHeight = mode === "list" ? 48 : 34;
  const height = Math.min(maxHeight, Math.max(rowHeight, rows.length * rowHeight));
  useEffect(() => { setScrollTop(0); if (scrollElement.current) scrollElement.current.scrollTop = 0; }, [mode]);
  const start = Math.min(Math.max(0, Math.floor(scrollTop / rowHeight) - 4), Math.max(0, rows.length - Math.ceil(height / rowHeight)));
  const end = Math.min(rows.length, start + Math.ceil(height / rowHeight) + 8);
  if (files.length === 0) return <p className="empty-group">No files</p>;
  return <div ref={scrollElement} className="woo-file-scroll" style={{ height }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
    <div className="woo-file-spacer" style={{ height: rows.length * rowHeight }}>
      {rows.slice(start, end).map((row, offset) => <div key={row.key} className={`woo-file-row ${row.file?.path === selectedPath ? "selected" : ""} ${row.directory ? "directory" : ""}`} style={{ top: (start + offset) * rowHeight, height: rowHeight, paddingLeft: 8 + row.depth * 15 }}>
        {row.directory ? <span className="woo-directory-name">▾ {row.directory}</span> : row.file && <>
          <button type="button" className="woo-file-select" disabled={busy || !onSelect} title={row.file.oldPath ? `${row.file.oldPath} → ${row.file.path}` : row.file.path} onClick={() => onSelect?.(row.file!)} onContextMenu={(event) => onContextMenu?.(row.file!, event)}>
            <span className={`woo-file-kind kind-${row.file.kind}`}>{row.file.kind.slice(0, 1).toUpperCase()}</span>
            <span className="woo-file-text"><strong>{row.file.path.split("/").at(-1)}</strong>{mode === "list" && <small>{row.file.oldPath ? `${row.file.oldPath} → ` : ""}{row.file.path.includes("/") ? `${row.file.path.slice(0, row.file.path.lastIndexOf("/"))}/` : "./"}</small>}</span>
          </button>
          {action && onAction && <button type="button" className="row-action" disabled={busy} onClick={() => onAction(row.file!)}>{action}</button>}
        </>}
      </div>)}
    </div>
  </div>;
}
