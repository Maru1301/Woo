import { useEffect, useRef, useState } from "react";
import { useRepositoryViewActive, useRepositoryViewId } from "../../app/repository-session/RepositoryView";
import { createTag, deleteTag, getTags, messageForError, type TagList, type TagMutationResult } from "../../lib/repository";

export default function TagPanel({ busy, onBusyChange, onMutation, onInconsistent, refreshToken = 0 }: {
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onMutation: (result: TagMutationResult) => void;
  onInconsistent: (message: string) => void;
  refreshToken?: number;
}) {
  const repositoryId = useRepositoryViewId();
  const viewActive = useRepositoryViewActive();
  const [list, setList] = useState<TagList | null>(null);
  const [name, setName] = useState("");
  const [annotated, setAnnotated] = useState(false);
  const [annotation, setAnnotation] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [working, setWorking] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const alive = useRef(true);
  const request = useRef(0);
  const locked = useRef(false);
  const previousRefresh = useRef(refreshToken);
  useEffect(() => {
    alive.current = viewActive;
    if (viewActive && (list === null || previousRefresh.current !== refreshToken)) {
      previousRefresh.current = refreshToken;
      void refresh();
    }
    return () => { alive.current = false; request.current += 1; };
  }, [viewActive, refreshToken]);
  async function refresh() {
    const id = ++request.current;
    setError("");
    try { const result = await getTags(repositoryId); if (alive.current && id === request.current) setList(result); }
    catch (cause) { if (alive.current && id === request.current) setError(messageForError(cause)); }
  }
  async function mutate(label: string, action: () => Promise<TagMutationResult>) {
    if (locked.current || busy) return;
    locked.current = true;
    onBusyChange(true);
    setWorking(label);
    setError("");
    setNotice("");
    const id = ++request.current;
    try {
      const result = await action();
      if (!alive.current || id !== request.current) return;
      setList(result.tags);
      setConfirmDelete(null);
      onMutation(result);
      setNotice(`${label} complete.`);
      if (label === "Creating tag") { setName(""); setAnnotation(""); }
    } catch (cause) {
      if (alive.current && id === request.current) {
        const detail = messageForError(cause);
        setError(detail);
        if (typeof cause === "object" && cause !== null && "code" in cause && cause.code === "tag_refresh_failed") onInconsistent(detail);
      }
    } finally {
      locked.current = false;
      onBusyChange(false);
      if (alive.current && id === request.current) setWorking("");
    }
  }
  return <section className="management-panel" aria-label="Tags">
    <div className="management-heading"><div><p className="eyebrow">REFS</p><h2>Tags</h2></div><button className="secondary" disabled={busy || !!working} onClick={() => void refresh()}>Refresh</button></div>
    <form className="management-form tag-form" onSubmit={(event) => { event.preventDefault(); if (name.trim()) void mutate("Creating tag", () => createTag(repositoryId, name.trim(), annotated ? annotation : null, null)); }}>
      <input aria-label="Tag name" placeholder="Tag name" value={name} onChange={(event) => setName(event.target.value)} disabled={busy || !!working} />
      <label><input type="checkbox" checked={annotated} onChange={(event) => setAnnotated(event.target.checked)} disabled={busy || !!working} /> Annotated</label>
      {annotated && <textarea aria-label="Tag annotation" placeholder="Annotation message" value={annotation} onChange={(event) => setAnnotation(event.target.value)} disabled={busy || !!working} rows={3} />}
      <button disabled={busy || !!working || !name.trim() || (annotated && !annotation.trim())}>Create at HEAD</button>
    </form>
    {list === null && !error && <p className="status-placeholder">Loading tags…</p>}
    {list?.tags.length === 0 && <p className="empty-group">No tags</p>}
    {list && list.tags.length > 0 && <div className="management-list" style={{ height: Math.min(300, list.tags.length * 55) }} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}><div className="management-spacer" style={{ height: list.tags.length * 55 }}>{list.tags.slice(Math.max(0, Math.floor(scrollTop / 55) - 3), Math.min(list.tags.length, Math.floor(scrollTop / 55) + 10)).map((tag, index) => <div className="management-row" style={{ position: "absolute", top: (Math.max(0, Math.floor(scrollTop / 55) - 3) + index) * 55, left: 0, right: 0, height: 55 }} key={tag.name}>
      <div className="management-description"><strong>{tag.name}</strong><small>{tag.kind} · {tag.targetHash.slice(0, 8)}</small></div>
      <button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmDelete === tag.name) void mutate("Deleting tag", () => deleteTag(repositoryId, tag.name)); else setConfirmDelete(tag.name); }}>{confirmDelete === tag.name ? "Confirm delete" : "Delete"}</button>
    </div>)}</div></div>}
    {working && <p className="operation-feedback" role="status">{working}…</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {notice && <p className="commit-notice" role="status">{notice}</p>}
  </section>;
}
