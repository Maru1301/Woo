import type { MouseEvent } from "react";
import type { FileChange } from "../../lib/repository";
import { FilePresentation, type FileViewMode } from "./FilePresentation";

export function FileGroup({ title, files, action, onAction, onSelect, onContextMenu, selectedPath, mode, busy }: {
  title: string;
  files: FileChange[];
  action?: string;
  onAction?: (change: FileChange) => void;
  onSelect?: (change: FileChange) => void;
  onContextMenu?: (change: FileChange, event: MouseEvent) => void;
  selectedPath?: string | null;
  mode: FileViewMode;
  busy: boolean;
}) {
  return <section className="file-group" aria-label={title}>
    <h3>{title}<span>{files.length}</span></h3>
    <FilePresentation files={files} mode={mode} selectedPath={selectedPath} busy={busy} action={action} onAction={onAction} onSelect={onSelect} onContextMenu={onContextMenu} />
  </section>;
}
