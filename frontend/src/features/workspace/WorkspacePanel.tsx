import { useState } from "react";
import type { WorkspaceCatalog } from "./workspace";

export function WorkspacePanel({ catalog, busy, error, onCreate, onRename, onDelete, onSwitch, onSelectRepository, onRemoveRepository }: {
  catalog: WorkspaceCatalog | null;
  busy: boolean;
  error: string;
  onCreate: (name: string) => void;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
  onSwitch: (id: string) => void;
  onSelectRepository: (workspaceId: string, repositoryId: string) => void;
  onRemoveRepository: (workspaceId: string, repositoryId: string) => void;
}) {
  const [name, setName] = useState("");
  const [rename, setRename] = useState("");
  const current = catalog?.workspaces.find((item) => item.id === catalog.activeWorkspaceId);
  return <section className="workspace-registry" aria-label="Workspaces">
    <h2>Workspaces</h2>
    {catalog === null && !error && <p>Loading workspaces…</p>}
    {catalog && <>
      <label htmlFor="workspace-select">Active workspace</label>
      <select id="workspace-select" value={catalog.activeWorkspaceId ?? ""} disabled={busy} onChange={(event) => onSwitch(event.target.value)}>
        {!catalog.activeWorkspaceId && <option value="">Choose a workspace</option>}
        {catalog.workspaces.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
      </select>
      {current && <>
        <div className="workspace-inline-form">
          <input aria-label="New workspace name" value={rename} onChange={(event) => setRename(event.target.value)} placeholder="Rename workspace" disabled={busy} />
          <button className="secondary" disabled={busy || !rename.trim()} onClick={() => { onRename(current.id, rename); setRename(""); }}>Rename</button>
          <button className="secondary" disabled={busy} onClick={() => { if (window.confirm(`Delete workspace “${current.name}”? Repositories on disk will not be deleted.`)) onDelete(current.id); }}>Delete workspace</button>
        </div>
        <h3>Repositories</h3>
        {current.repositories.length === 0 && <p>No repositories registered here.</p>}
        {current.repositories.map((repository) => <div className="workspace-repository" key={repository.id}>
          <button className="secondary" disabled={busy} aria-current={current.activeRepositoryId === repository.id ? "true" : undefined} onClick={() => onSelectRepository(current.id, repository.id)}>{repository.path.split(/[\\/]/).filter(Boolean).at(-1) ?? repository.path}</button>
          <span title={repository.path}>{repository.path}</span>
          <button className="secondary" disabled={busy} onClick={() => { if (window.confirm(`Remove “${repository.path}” from this workspace? Files on disk will not be deleted.`)) onRemoveRepository(current.id, repository.id); }}>Remove</button>
        </div>)}
      </>}
      <div className="workspace-inline-form">
        <input aria-label="Workspace name" value={name} onChange={(event) => setName(event.target.value)} placeholder="New workspace name" disabled={busy} onKeyDown={(event) => { if (event.key === "Enter" && name.trim()) { onCreate(name); setName(""); } }} />
        <button disabled={busy || !name.trim()} onClick={() => { onCreate(name); setName(""); }}>Create workspace</button>
      </div>
    </>}
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
