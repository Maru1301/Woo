import { useEffect, useRef, useState } from "react";
import { cancelRemoteOperation, getRemoteOperation, getRemotes, messageForError, startFetch, startPull, startPush, type RemoteInfo, type RemoteKind, type RemoteOperationStatus, type RemoteRefresh } from "../../lib/repository";

export default function RemotePanel({ onComplete, onInconsistent, onBusyChange, localBusy }: {
  onComplete: (refresh: RemoteRefresh) => void;
  onInconsistent: (message: string) => void;
  onBusyChange: (busy: boolean) => void;
  localBusy: boolean;
}) {
  const [remotes, setRemotes] = useState<RemoteInfo[]>([]);
  const [remote, setRemote] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [operation, setOperation] = useState<RemoteOperationStatus | null>(null);
  const [notice, setNotice] = useState("");
  const alive = useRef(true);
  const active = useRef(false);
  const callbacks = useRef({ onComplete, onInconsistent, onBusyChange });
  callbacks.current = { onComplete, onInconsistent, onBusyChange };

  useEffect(() => {
    alive.current = true;
    void getRemotes().then((list) => {
      if (!alive.current) return;
      setRemotes(list.remotes);
      setRemote((current) => current || list.remotes[0]?.name || "");
    }).catch((cause) => { if (alive.current) setError(messageForError(cause)); })
      .finally(() => { if (alive.current) setLoading(false); });
    return () => { alive.current = false; callbacks.current.onBusyChange(false); };
  }, []);

  async function poll(id: number) {
    while (alive.current) {
      await new Promise((resolve) => setTimeout(resolve, 300));
      if (!alive.current) break;
      let next: RemoteOperationStatus;
      try { next = await getRemoteOperation(id); }
      catch (cause) { if (alive.current) setError(messageForError(cause)); break; }
      if (!alive.current) break;
      setOperation(next);
      if (next.phase === "completed" || next.phase === "failed" || next.phase === "cancelled" || next.phase === "timed_out") {
        if (next.refresh) callbacks.current.onComplete(next.refresh);
        else if (next.error?.code === "remote_refresh_failed") callbacks.current.onInconsistent(next.error.message);
      }
      if (next.phase === "completed") {
        setNotice(`${next.kind[0].toUpperCase()}${next.kind.slice(1)} complete.`);
        break;
      }
      if (next.phase === "failed" || next.phase === "cancelled" || next.phase === "timed_out") {
        setError(next.error?.message || "The remote operation stopped.");
        break;
      }
    }
    active.current = false;
    if (alive.current) callbacks.current.onBusyChange(false);
  }

  async function run(kind: RemoteKind) {
    if (active.current || localBusy) return;
    active.current = true;
    callbacks.current.onBusyChange(true);
    setError("");
    setNotice("");
    try {
      const started = kind === "fetch" ? await startFetch(remote) : kind === "pull" ? await startPull() : await startPush();
      if (!alive.current) return;
      setOperation(started);
      void poll(started.id);
    } catch (cause) {
      if (alive.current) setError(messageForError(cause));
      active.current = false;
      callbacks.current.onBusyChange(false);
    }
  }

  const running = operation?.phase === "queued" || operation?.phase === "running";
  return <section className="remote-panel" aria-label="Remote operations">
    <div className="remote-heading"><div><p className="eyebrow">REMOTES</p><h2>Sync</h2></div>
      <div className="remote-actions">
        <select aria-label="Fetch remote" value={remote} disabled={running || loading || localBusy} onChange={(event) => setRemote(event.target.value)}>
          {remotes.map((item) => <option key={item.name} value={item.name}>{item.name}</option>)}
        </select>
        <button className="secondary" disabled={!remote || running || localBusy} onClick={() => void run("fetch")}>Fetch</button>
        <button className="secondary" disabled={running || localBusy || remotes.length === 0} onClick={() => void run("pull")}>Pull</button>
        <button disabled={running || localBusy || remotes.length === 0} onClick={() => void run("push")}>Push</button>
      </div>
    </div>
    {loading && <p className="status-placeholder">Loading remotes…</p>}
    {!loading && remotes.length === 0 && !error && <p className="status-placeholder">No remotes configured.</p>}
    {running && <p className="operation-feedback" role="status">{operation.kind === "fetch" ? "Fetching" : operation.kind === "pull" ? "Pulling" : "Pushing"}… {Math.floor(operation.elapsedMs / 1000)}s <button className="secondary" onClick={() => void cancelRemoteOperation(operation.id).catch((cause) => setError(messageForError(cause)))}>Cancel</button></p>}
    {notice && <p className="commit-notice" role="status">{notice}</p>}
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
