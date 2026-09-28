import { useEffect, useRef, useState } from "react";
import { useRepositoryViewActive, useRepositoryViewId } from "../../app/repository-session/RepositoryView";
import { applyStash, createStash, dropStash, getStashes, messageForError, popStash, type StashList, type StashMutationResult } from "../../lib/repository";

export default function StashPanel({ busy, onBusyChange, onMutation, onInconsistent, refreshToken = 0 }: {
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onMutation: (result: StashMutationResult) => void;
  onInconsistent: (message: string) => void;
  refreshToken?: number;
}) {
  const repositoryId = useRepositoryViewId();
  const viewActive = useRepositoryViewActive();
  const [list, setList] = useState<StashList | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [working, setWorking] = useState("");
  const [confirmDrop, setConfirmDrop] = useState<string | null>(null);
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
    try {
      const result = await getStashes(repositoryId);
      if (alive.current && id === request.current) setList(result);
    } catch (cause) {
      if (alive.current && id === request.current) setError(messageForError(cause));
    }
  }
  async function mutate(label: string, action: () => Promise<StashMutationResult>) {
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
      setList(result.stashes);
      setConfirmDrop(null);
      onMutation(result);
      if (result.error) setError(result.error.message);
      else { setNotice(`${label} complete.`); if (label === "Stashing") setMessage(""); }
    } catch (cause) {
      if (alive.current && id === request.current) {
        const detail = messageForError(cause);
        setError(detail);
        if (typeof cause === "object" && cause !== null && "code" in cause && cause.code === "stash_refresh_failed") onInconsistent(detail);
      }
    } finally {
      locked.current = false;
      onBusyChange(false);
      if (alive.current && id === request.current) setWorking("");
    }
  }
  return <section className="management-panel" aria-label="Stashes">
    <div className="management-heading"><div><p className="eyebrow">LOCAL CHANGES</p><h2>Stashes</h2></div><button className="secondary" disabled={busy || !!working} onClick={() => void refresh()}>Refresh</button></div>
    <p className="management-hint">Stash tracked and staged changes. Untracked files stay in the working tree.</p>
    <form className="management-form" onSubmit={(event) => { event.preventDefault(); void mutate("Stashing", () => createStash(repositoryId, message || null)); }}><input aria-label="Stash message" placeholder="Optional stash message" value={message} onChange={(event) => setMessage(event.target.value)} disabled={busy || !!working} /><button disabled={busy || !!working}>Stash changes</button></form>
    {list === null && !error && <p className="status-placeholder">Loading stashes…</p>}
    {list?.stashes.length === 0 && <p className="empty-group">No stashes</p>}
    {list && list.stashes.length > 0 && <div className="management-list">{list.stashes.map((stash) => <div className="management-row" key={stash.commitHash}>
      <div className="management-description"><strong>{stash.message}</strong><small>{stash.reference} · {stash.commitHash.slice(0, 8)}</small></div>
      <button className="secondary" disabled={busy || !!working} onClick={() => void mutate("Applying stash", () => applyStash(repositoryId, stash.commitHash))}>Apply</button>
      <button className="secondary" disabled={busy || !!working} onClick={() => void mutate("Popping stash", () => popStash(repositoryId, stash.commitHash))}>Pop</button>
      <button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmDrop === stash.commitHash) void mutate("Dropping stash", () => dropStash(repositoryId, stash.commitHash)); else setConfirmDrop(stash.commitHash); }}>{confirmDrop === stash.commitHash ? "Confirm drop" : "Drop"}</button>
    </div>)}</div>}
    {working && <p className="operation-feedback" role="status">{working}…</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {notice && <p className="commit-notice" role="status">{notice}</p>}
  </section>;
}
