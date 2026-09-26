import { useEffect, useState } from "react";
import { getOperationHistory, messageForError, type OperationEntry } from "../../lib/repository";

export default function OperationHistory({ active }: { active: boolean }) {
  const [entries, setEntries] = useState<OperationEntry[]>([]);
  const [filter, setFilter] = useState<"all" | "user" | "background">("all");
  const [error, setError] = useState("");
  useEffect(() => {
    if (!active) return;
    let live = true;
    const update = () => void getOperationHistory().then((items) => {
      if (live) { setEntries(items); setError(""); }
    }).catch((cause) => { if (live) setError(messageForError(cause)); });
    update();
    const timer = window.setInterval(update, 2000);
    return () => { live = false; window.clearInterval(timer); };
  }, [active]);

  const visible = entries.filter((entry) => filter === "all" || entry.source === filter);
  return <section className="operation-history" aria-label="Git activity">
    <div className="operation-history-heading"><div><p className="eyebrow">ACTIVITY</p><h2>Operation history</h2></div>
      <select aria-label="Filter activity" value={filter} onChange={(event) => setFilter(event.target.value as typeof filter)}>
        <option value="all">All</option><option value="user">User</option><option value="background">Background</option>
      </select>
    </div>
    {error && <p className="error" role="alert">{error}</p>}
    {!error && visible.length === 0 && <p className="status-placeholder">No activity yet.</p>}
    <ol className="operation-history-list">{visible.map((entry) => <li key={entry.id}>
      <div><strong>{entry.kind}</strong> <span>{entry.source === "background" ? "Background" : "User"}</span> <span>{entry.phase.replace("_", " ")}</span> <span>{entry.repositoryId.split(/[\\/]/).filter(Boolean).at(-1)}</span></div>
      <small>{new Date(entry.startedAtMs).toLocaleString()} | {entry.durationMs == null ? "Running" : `${entry.durationMs} ms`}</small>
      {entry.diagnostics && <p className="error">{entry.diagnostics}</p>}
    </li>)}</ol>
  </section>;
}
