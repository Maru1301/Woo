import { useEffect, useRef, useState } from "react";
import {
  abortMerge, abortHistoryOperation, completeMerge, continueHistoryOperation, deleteConflict, getConflictContent, mergeBranch, messageForError, rebaseOnto, skipHistoryOperation,
  saveConflictText, stageConflict, useConflictSide,
  type BranchInfo, type ConflictContent, type ConflictFile, type ConflictMutationResult,
  type HistoryMutationResult, type MergeMutationResult, type RepositoryOperation,
} from "../../lib/repository";

function preview(part: ConflictContent["ours"], label: string) {
  if (!part) return <p className="conflict-unavailable">{label} does not contain this file.</p>;
  if (part.oversized) return <p className="conflict-unavailable">{label} exceeds the 256 KiB text limit. Choose a whole-file side or resolve it externally.</p>;
  if (part.isBinary) return <p className="conflict-unavailable">{label} is binary or a non-regular file.</p>;
  const text = part.text ?? "";
  return <pre>{text.length > 20_000 ? `${text.slice(0, 20_000)}\n… preview truncated` : text}</pre>;
}

export default function MergePanel({ branches, operation, conflicts, busy, refreshToken, onBusyChange, onMerge, onHistory, onConflict, onInconsistent }: {
  branches: BranchInfo[];
  operation: RepositoryOperation;
  conflicts: ConflictFile[];
  busy: boolean;
  refreshToken: number;
  onBusyChange: (busy: boolean) => void;
  onMerge: (result: MergeMutationResult) => void;
  onHistory: (result: HistoryMutationResult) => void;
  onConflict: (result: ConflictMutationResult) => void;
  onInconsistent: (message: string) => void;
}) {
  const [target, setTarget] = useState("");
  const [rebaseTarget, setRebaseTarget] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<ConflictContent | null>(null);
  const [contentKey, setContentKey] = useState("");
  const [editor, setEditor] = useState("");
  const [mergeMessage, setMergeMessage] = useState("");
  const [loadingContent, setLoadingContent] = useState(false);
  const [working, setWorking] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [confirmAbort, setConfirmAbort] = useState(false);
  const [confirmSkip, setConfirmSkip] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const alive = useRef(true);
  const contentRequest = useRef(0);
  const locked = useRef(false);
  const callbacks = useRef({ onBusyChange, onMerge, onHistory, onConflict, onInconsistent });
  callbacks.current = { onBusyChange, onMerge, onHistory, onConflict, onInconsistent };

  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; contentRequest.current += 1; };
  }, []);
  useEffect(() => {
    if (operation.kind === "merge") setMergeMessage(operation.message);
  }, [operation.kind, operation.kind === "merge" ? operation.message : ""]);
  useEffect(() => {
    if (!selected || !conflicts.some((file) => file.path === selected)) setSelected(conflicts[0]?.path ?? null);
  }, [conflicts, selected]);
  useEffect(() => { setConfirmDelete(null); }, [selected]);
  useEffect(() => {
    const request = ++contentRequest.current;
    setContent(null);
    setContentKey("");
    setEditor("");
    if (!selected) { setLoadingContent(false); return; }
    setLoadingContent(true);
    void getConflictContent(selected).then((value) => {
      if (!alive.current || request !== contentRequest.current) return;
      setContent(value);
      setContentKey(`${selected}:${refreshToken}`);
      setEditor(value.working?.text ?? "");
    }).catch((cause) => {
      if (alive.current && request === contentRequest.current) setError(messageForError(cause));
    }).finally(() => {
      if (alive.current && request === contentRequest.current) setLoadingContent(false);
    });
  }, [selected, refreshToken]);

  async function runMerge(label: string, action: () => Promise<MergeMutationResult>) {
    if (locked.current || busy) return;
    locked.current = true;
    callbacks.current.onBusyChange(true);
    setWorking(label);
    setError("");
    setNotice("");
    try {
      const result = await action();
      if (!alive.current) return;
      callbacks.current.onMerge(result);
      setConfirmAbort(false);
      if (result.error) setError(result.error.message);
      setNotice(({
        already_up_to_date: "Already up to date.", fast_forward: "Fast-forward complete.", clean_merge: "Merge complete.",
        needs_resolution: "Merge stopped for conflicts. Resolve and stage each file.", needs_completion: "Merge ready to complete.",
        failed: "Merge did not complete.", completed: "Merge commit created.", aborted: "Merge aborted.",
      })[result.outcome]);
    } catch (cause) {
      if (!alive.current) return;
      const detail = messageForError(cause);
      setError(detail);
      if (typeof cause === "object" && cause !== null && "code" in cause && cause.code === "merge_refresh_failed") callbacks.current.onInconsistent(detail);
    } finally {
      locked.current = false;
      callbacks.current.onBusyChange(false);
      if (alive.current) setWorking("");
    }
  }

  async function runConflict(label: string, action: () => Promise<ConflictMutationResult>) {
    if (locked.current || busy) return;
    locked.current = true;
    callbacks.current.onBusyChange(true);
    setWorking(label);
    setError("");
    setNotice("");
    try {
      const result = await action();
      if (!alive.current) return;
      callbacks.current.onConflict(result);
      setConfirmDelete(null);
      setConfirmAbort(false);
      if (result.error) setError(result.error.message);
      else setNotice(`${label} complete.`);
    } catch (cause) {
      if (!alive.current) return;
      const detail = messageForError(cause);
      setError(detail);
      if (typeof cause === "object" && cause !== null && "code" in cause && cause.code === "merge_refresh_failed") callbacks.current.onInconsistent(detail);
    } finally {
      locked.current = false;
      callbacks.current.onBusyChange(false);
      if (alive.current) setWorking("");
    }
  }

  async function runHistory(label: string, action: () => Promise<HistoryMutationResult>) {
    if (locked.current || busy) return;
    locked.current = true;
    callbacks.current.onBusyChange(true);
    setWorking(label); setError(""); setNotice("");
    try {
      const result = await action();
      if (!alive.current) return;
      callbacks.current.onHistory(result);
      setConfirmAbort(false); setConfirmSkip(false);
      if (result.error) setError(result.error.message);
      else setNotice(result.state.operation.kind === "none" ? `${label} complete.` : `${label} stopped. Resolve and stage conflicts before continuing.`);
    } catch (cause) {
      if (!alive.current) return;
      const detail = messageForError(cause);
      setError(detail);
      if (typeof cause === "object" && cause !== null && "code" in cause && cause.code === "merge_refresh_failed") callbacks.current.onInconsistent(detail);
    } finally {
      locked.current = false; callbacks.current.onBusyChange(false); if (alive.current) setWorking("");
    }
  }

  const localBranches = branches.filter((branch) => branch.kind === "local" && !branch.isCurrent);
  const active = operation.kind === "merge";
  const historyActive = operation.kind === "rebase" || operation.kind === "cherry_pick" || operation.kind === "revert";
  const rebaseLabels = operation.kind === "rebase";
  const context = operation.kind === "rebase" ? `Rebase in progress${operation.step && operation.total ? ` (${operation.step}/${operation.total})` : ""}${operation.currentCommit ? ` · Applying ${operation.currentCommit.slice(0, 10)}` : ""}`
    : operation.kind === "cherry_pick" ? `Cherry-pick in progress · ${operation.commit.slice(0, 10)}`
    : operation.kind === "revert" ? `Revert in progress · ${operation.commit.slice(0, 10)}` : "Merge in progress";
  const currentContent = contentKey === `${selected}:${refreshToken}` ? content : null;
  const editable = !!currentContent && (!currentContent.working || (!currentContent.working.isBinary && !currentContent.working.oversized));
  const editorChanged = editable && editor !== (currentContent?.working?.text ?? "");
  return <section className="merge-panel" aria-label="Merge and conflicts">
    <div className="merge-heading"><div><p className="eyebrow">REPOSITORY OPERATION</p><h2>{active || historyActive ? context : "Merge or rebase"}</h2></div><span>{conflicts.length} unresolved</span></div>
    {!active && !historyActive && <><div className="merge-start"><label htmlFor="merge-target">Merge local branch into current branch</label><div><select id="merge-target" value={target} onChange={(event) => setTarget(event.target.value)} disabled={busy || !!working || conflicts.length > 0}><option value="">Choose branch</option>{localBranches.map((branch) => <option value={branch.fullRefName} key={branch.fullRefName}>{branch.name}</option>)}</select><button disabled={busy || !!working || !target || conflicts.length > 0} onClick={() => void runMerge("Merging", () => mergeBranch(target))}>Merge</button></div></div>
      <div className="merge-start"><label htmlFor="rebase-target">Rebase current branch onto local branch</label><div><select id="rebase-target" value={rebaseTarget} onChange={(event) => setRebaseTarget(event.target.value)} disabled={busy || !!working || conflicts.length > 0}><option value="">Choose branch</option>{localBranches.map((branch) => <option value={branch.fullRefName} key={branch.fullRefName}>{branch.name}</option>)}</select><button disabled={busy || !!working || !rebaseTarget || conflicts.length > 0} onClick={() => void runHistory("Rebase", () => rebaseOnto(rebaseTarget))}>Rebase</button></div></div></>}
    {active && <div className="merge-complete"><p>Merge targets: {operation.mergeHeads.map((hash) => hash.slice(0, 10)).join(", ")}. Resolve and stage every conflict before completing.</p><label htmlFor="merge-message">Merge commit message</label><textarea id="merge-message" value={mergeMessage} onChange={(event) => setMergeMessage(event.target.value)} disabled={busy || !!working} rows={4} /><p>Review Git's prepared message before committing; any comment lines shown here will be saved as text.</p><div><button disabled={busy || !!working || conflicts.length > 0 || !mergeMessage.trim()} onClick={() => void runMerge("Completing merge", () => completeMerge(mergeMessage))}>Complete merge</button><button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmAbort) void runMerge("Aborting merge", abortMerge); else setConfirmAbort(true); }}>{confirmAbort ? "Confirm abort (discard resolutions)" : "Abort merge"}</button></div></div>}
    {historyActive && <div className="merge-complete"><p>Resolve and stage each conflict, then continue. Git may stop again at another commit.</p><div><button disabled={busy || !!working || conflicts.length > 0} onClick={() => void runHistory("Continue", continueHistoryOperation)}>Continue</button><button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmSkip) void runHistory("Skip current commit", skipHistoryOperation); else setConfirmSkip(true); }}>{confirmSkip ? "Confirm skip (omit this commit)" : "Skip current commit"}</button><button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmAbort) void runHistory("Abort", abortHistoryOperation); else setConfirmAbort(true); }}>{confirmAbort ? "Confirm abort (discard resolutions)" : `Abort ${operation.kind.replace("_", "-")}`}</button></div></div>}
    {conflicts.length > 0 && <div className="conflict-area"><div className="conflict-list"><h3>Conflicted files</h3>{conflicts.map((file) => <button className={selected === file.path ? "selected" : ""} key={file.path} onClick={() => { setSelected(file.path); setError(""); }}><span>{file.path}</span><small>{file.kind.replaceAll("_", " ")}</small></button>)}</div>
      <div className="conflict-detail"><h3>{selected ?? "Select a conflict"}</h3>{loadingContent && <p className="status-placeholder">Loading conflict sides…</p>}{currentContent && <>
        <p className="management-hint">Choosing a side replaces the working file and stages it. Save edited text before staging.</p>
        {rebaseLabels && <p className="management-hint">During rebase, “ours” is the branch being rebased onto plus already replayed commits; “theirs” is the commit currently being replayed.</p>}
        <div className="conflict-sides"><div><h4>{rebaseLabels ? "Ours · onto branch" : "Ours"}</h4>{preview(currentContent.ours, "Ours")}</div><div><h4>{rebaseLabels ? "Theirs · replayed commit" : "Theirs"}</h4>{preview(currentContent.theirs, "Theirs")}</div><div><h4>Base</h4>{preview(currentContent.base, "Base")}</div></div>
        <div className="conflict-side-actions"><button className="secondary" disabled={busy || !!working || !selected} onClick={() => void runConflict("Using ours", () => useConflictSide(selected!, "ours"))}>Use ours{!currentContent.ours ? " (delete)" : ""} and stage</button><button className="secondary" disabled={busy || !!working || !selected} onClick={() => void runConflict("Using theirs", () => useConflictSide(selected!, "theirs"))}>Use theirs{!currentContent.theirs ? " (delete)" : ""} and stage</button></div>
        <h4>Working copy</h4>{editable ? <><textarea className="conflict-editor" aria-label="Resolved file content" value={editor} onChange={(event) => setEditor(event.target.value)} disabled={busy || !!working} rows={14} spellCheck={false} /><div className="conflict-side-actions"><button disabled={busy || !!working || !editorChanged} onClick={() => void runConflict("Saving resolution", () => saveConflictText(selected!, currentContent.working?.text ?? null, editor))}>Save working copy</button><button className="secondary" disabled={busy || !!working || editorChanged} onClick={() => void runConflict("Staging resolution", () => stageConflict(selected!))}>Stage resolved file</button></div></> : <p className="conflict-unavailable">Working copy is binary, non-regular, or exceeds 256 KiB. Choose a side or resolve externally and refresh.</p>}
        <div className="conflict-side-actions"><button className="secondary" disabled={busy || !!working} onClick={() => { if (confirmDelete === selected) void runConflict("Resolving as deleted", () => deleteConflict(selected!)); else setConfirmDelete(selected); }}>{confirmDelete === selected ? "Confirm delete and stage" : "Resolve as deleted"}</button></div>
      </>}</div></div>}
    {working && <p className="operation-feedback" role="status">{working}…</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {notice && <p className="commit-notice" role="status">{notice}</p>}
  </section>;
}
