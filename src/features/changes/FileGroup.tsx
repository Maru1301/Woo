import { useState } from "react";
import type { FileChange } from "../../lib/repository";

const ROW_HEIGHT = 44;

export function FileGroup({ title, files, action, onAction, onSelect, busy }: {
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
