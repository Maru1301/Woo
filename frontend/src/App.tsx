import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import HistoryPanel from "./features/history/HistoryPanel";
import RemotePanel from "./features/remotes/RemotePanel";
import BranchPanel from "./features/branches/BranchPanel";
import StashPanel from "./features/stash/StashPanel";
import TagPanel from "./features/tags/TagPanel";
import MergePanel from "./features/operations/MergePanel";
import OperationHistory from "./features/operations/OperationHistory";
import DiffViewer, { type DiffSource } from "./features/diff/DiffViewer";
import { useRepositorySession } from "./app/repository-session/useRepositorySession";
import { FileGroup } from "./features/changes/FileGroup";
import { FileViewToggle, type FileViewMode } from "./features/changes/FilePresentation";
import { ContextMenu } from "./components/ui/ContextMenu";
import { WorkspaceLayout, type WorkspaceView } from "./layout/WorkspaceLayout";
import { WorkspacePanel } from "./features/workspace/WorkspacePanel";
import { createWorkspace, deleteWorkspace, registerRepository, removeRepository, renameWorkspace, restoreWorkspaceSession, switchWorkspace, switchWorkspaceRepository, type WorkspaceCatalog, type WorkspaceTransition } from "./features/workspace/workspace";
import {
  checkoutBranch, commitStaged, createBranch, deleteBranch, errorCode, getBranches, getRepositoryState, messageForError, partialStage, renameBranch, revalidateRepository, stageAll, stageFile,
  cherryPick, createTag, rebaseOnto, resetTo, revertCommit, unstageAll, unstageFile, type AutoRefreshEvent, type ConflictMutationResult, type FileChange, type HistoryMutationResult, type MergeMutationResult, type PartialSelection, type PartialStageResult, type RemoteRefresh, type RepositoryState, type RepositoryStatus, type ResetMode, type StashMutationResult,
} from "./lib/repository";

export default function App() {
  const [path, setPath] = useState("");
  const [workspaceCatalog, setWorkspaceCatalog] = useState<WorkspaceCatalog | null>(null);
  const [workspaceError, setWorkspaceError] = useState("");
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [activeView, setActiveView] = useState<WorkspaceView>("history");
  const [localFilesMode, setLocalFilesMode] = useState<FileViewMode>("list");
  const [fileMenu, setFileMenu] = useState<{ x: number; y: number; source: "staged" | "unstaged" | "untracked"; change: FileChange } | null>(null);
  const [showOpen, setShowOpen] = useState(true);
  const [session, dispatchSession] = useRepositorySession();
  const { repository, status, branches, operation, conflicts } = session;
  const [openError, setOpenError] = useState("");
  const [conflictVersion, setConflictVersion] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [commitMessage, setCommitMessage] = useState("");
  const [commitError, setCommitError] = useState("");
  const [commitNotice, setCommitNotice] = useState("");
  const [selectedChange, setSelectedChange] = useState<{ source: DiffSource; change: FileChange } | null>(null);
  const [branchError, setBranchError] = useState("");
  const [historyVersion, setHistoryVersion] = useState(0);
  const [historyRefreshVersion, setHistoryRefreshVersion] = useState(0);
  const [tagRefreshVersion, setTagRefreshVersion] = useState(0);
  const [diffRefreshVersion, setDiffRefreshVersion] = useState(0);
  const [historyActionError, setHistoryActionError] = useState("");
  const [historyActionNotice, setHistoryActionNotice] = useState("");
  const [remoteBusy, setRemoteBusy] = useState(false);
  const [managementBusy, setManagementBusy] = useState(false);
  const managementBusyRef = useRef(0);
  const [sessionVersion, setSessionVersion] = useState(0);
  const branchRequest = useRef(0);
  const repositoryRequest = useRef(0);
  const restoreStarted = useRef(false);
  const busyRef = useRef(false);
  const repositoryRef = useRef(repository);
  const lastWatchSequence = useRef(0);
  const pendingWatch = useRef<AutoRefreshEvent | null>(null);
  const pendingBackgroundRefresh = useRef(false);
  const remoteBusyRef = useRef(remoteBusy);
  repositoryRef.current = repository;
  remoteBusyRef.current = remoteBusy;

  function applyWatchEvent(event: AutoRefreshEvent) {
    const current = repositoryRef.current;
    if (!current || event.sessionId !== current.sessionId || event.sequence <= lastWatchSequence.current) return;
    lastWatchSequence.current = event.sequence;
    setFileMenu(null);
    if (event.unavailable) {
      dispatchSession({ type: "statusError", message: event.unavailable.message, clearOperation: true });
      setSelectedChange(null);
      return;
    }
    if (event.state) {
      dispatchSession({ type: "repositoryState", state: event.state });
      if (event.state.operation.kind !== operation.kind
        || event.state.conflicts.length !== conflicts.length
        || (event.clearDiff && (conflicts.length > 0 || event.state.conflicts.length > 0))
        || event.diffPaths.some((path) => event.state!.conflicts.some((file) => file.path === path))) {
        setConflictVersion((value) => value + 1);
      }
    }
    if (event.branch !== undefined && event.head !== undefined) {
      dispatchSession({ type: "watchIdentity", branch: event.branch, head: event.head });
    }
    if (event.branches) {
      branchRequest.current += 1;
      dispatchSession({ type: "branchesReady", branches: event.branches });
    }
    const actualHeadChanged = event.resetHistory && (current.head?.hash !== event.head?.hash || current.branch !== event.branch);
    if (actualHeadChanged) setHistoryVersion((value) => value + 1);
    else if (event.refreshHistory) setHistoryRefreshVersion((value) => value + 1);
    if (event.refreshTags) setTagRefreshVersion((value) => value + 1);
    if (event.clearDiff || actualHeadChanged) setSelectedChange(null);
    else if (selectedChange) {
      const group = event.state?.status[selectedChange.source as "staged" | "unstaged" | "untracked"];
      if (group && !group.some((change) => change.path === selectedChange.change.path)) setSelectedChange(null);
      else if (event.diffPaths.some((path) => path === selectedChange.change.path || selectedChange.change.path.startsWith(`${path}/`))) {
        setDiffRefreshVersion((value) => value + 1);
      }
    }
  }

  const watchHandler = useRef(applyWatchEvent);
  watchHandler.current = applyWatchEvent;
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<AutoRefreshEvent>("repository-auto-refresh", ({ payload }) => {
      if (busyRef.current || managementBusyRef.current > 0 || remoteBusyRef.current) {
        pendingWatch.current = payload;
      } else watchHandler.current(payload);
    }).then((stop) => { if (cancelled) stop(); else unlisten = stop; }).catch(() => {});
    return () => { cancelled = true; unlisten?.(); };
  }, []);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<{ sessionId: number; operationId: number; refresh: RemoteRefresh | null }>("background-fetch-completed", ({ payload }) => {
      if (repositoryRef.current?.sessionId !== payload.sessionId || !payload.refresh) return;
      if (busyRef.current || managementBusyRef.current > 0 || remoteBusyRef.current) {
        pendingBackgroundRefresh.current = true;
      } else applyRemoteRefresh(payload.refresh);
    }).then((stop) => { if (cancelled) stop(); else unlisten = stop; }).catch(() => {});
    return () => { cancelled = true; unlisten?.(); };
  }, []);
  useEffect(() => {
    if (busy || managementBusy || remoteBusy || (!pendingWatch.current && !pendingBackgroundRefresh.current)) return;
    pendingWatch.current = null;
    pendingBackgroundRefresh.current = false;
    void revalidateRepository();
  }, [busy, managementBusy, remoteBusy]);

  function applyRepositoryState(state: RepositoryState) {
    dispatchSession({ type: "repositoryState", state });
    setFileMenu(null);
    setConflictVersion((value) => value + 1);
  }

  async function loadBranches() {
    const request = ++branchRequest.current;
    setBranchError("");
    dispatchSession({ type: "branchesLoading" });
    try {
      const data = await getBranches();
      if (request === branchRequest.current) dispatchSession({ type: "branchesReady", branches: data });
    } catch (cause) {
      if (request === branchRequest.current) dispatchSession({ type: "branchesError", message: messageForError(cause) });
    }
  }

  function resetRepositorySession() {
    repositoryRequest.current += 1;
    setPath("");
    repositoryRef.current = null;
    dispatchSession({ type: "reset" });
    lastWatchSequence.current = 0;
    pendingWatch.current = null;
    pendingBackgroundRefresh.current = false;
    setRemoteBusy(false);
    setManagementBusy(false);
    branchRequest.current += 1;
    setBranchError("");
    setSelectedChange(null);
    setFileMenu(null);
    setCommitMessage("");
    setCommitError("");
    setCommitNotice("");
    setHistoryActionError(""); setHistoryActionNotice("");
  }

  async function attachRepository(info: NonNullable<WorkspaceTransition["repository"]>) {
    const request = repositoryRequest.current;
    repositoryRef.current = info;
    dispatchSession({ type: "opened", repository: info });
    setShowOpen(false);
    setActiveView("history");
    setSessionVersion((value) => value + 1);
    setHistoryVersion((value) => value + 1);
    void loadBranches();
    setPath(info.path);
    dispatchSession({ type: "statusLoading" });
    const statusSequence = lastWatchSequence.current;
    try {
      const initialState = await getRepositoryState();
      if (request === repositoryRequest.current && statusSequence === lastWatchSequence.current) applyRepositoryState(initialState);
    } catch (cause) {
      if (request === repositoryRequest.current && statusSequence === lastWatchSequence.current) dispatchSession({ type: "statusError", message: messageForError(cause) });
    }
  }

  async function runWorkspaceTransition(label: string, action: () => Promise<WorkspaceTransition>) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy) return;
    busyRef.current = true;
    setWorkspaceBusy(true);
    setBusy(label);
    setWorkspaceError("");
    setOpenError("");
    try {
      const result = await action();
      setWorkspaceCatalog(result.catalog);
      if (result.sessionChanged) {
        resetRepositorySession();
        if (result.repository) await attachRepository(result.repository);
        else { setShowOpen(true); if (result.openError) setOpenError(result.openError.message); }
      } else if (result.openError) setOpenError(result.openError.message);
    } catch (cause) {
      setWorkspaceError(messageForError(cause));
    } finally {
      busyRef.current = false;
      setWorkspaceBusy(false);
      setBusy(null);
    }
  }

  useEffect(() => {
    if (restoreStarted.current) return;
    restoreStarted.current = true;
    void runWorkspaceTransition("Restoring workspace", restoreWorkspaceSession);
  }, []);

  async function loadRepository(selectedPath: string) {
    const workspaceId = workspaceCatalog?.activeWorkspaceId;
    if (!selectedPath.trim() || !workspaceId) return;
    await runWorkspaceTransition("Adding repository", () => registerRepository(workspaceId, selectedPath.trim()));
  }

  async function renameCurrentWorkspace(id: string, name: string) {
    if (busyRef.current) return;
    busyRef.current = true;
    setWorkspaceBusy(true);
    setWorkspaceError("");
    try { setWorkspaceCatalog(await renameWorkspace(id, name)); }
    catch (cause) { setWorkspaceError(messageForError(cause)); }
    finally { busyRef.current = false; setWorkspaceBusy(false); }
  }

  async function chooseRepository() {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy) return;
    try {
      const selected = await open({ directory: true, multiple: false, title: "Open Git repository" });
      if (selected) {
        setPath(selected);
        await loadRepository(selected);
      }
    } catch (cause) {
      setOpenError(messageForError(cause));
    }
  }

  async function refresh() {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository) return;
    busyRef.current = true;
    setBusy("Refreshing changes");
    dispatchSession({ type: "statusLoading" });
    setSelectedChange(null);
    setFileMenu(null);
    try {
      applyRepositoryState(await getRepositoryState());
    } catch (cause) {
      dispatchSession({ type: "statusError", message: messageForError(cause), clearOperation: true });
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function changeIndex(label: string, operation: () => Promise<RepositoryStatus>) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return;
    busyRef.current = true;
    setBusy(label);
    setSelectedChange(null);
    setCommitNotice("");
    try {
      dispatchSession({ type: "statusReady", status: await operation() });
      setCommitError("");
    } catch (cause) {
      dispatchSession({ type: "statusError", message: messageForError(cause) });
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function changePart(source: "staged" | "unstaged", change: FileChange, selection: PartialSelection): Promise<PartialStageResult> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) throw new Error("Another repository operation is running.");
    busyRef.current = true;
    setBusy(source === "staged" ? "Unstaging selected changes" : "Staging selected changes");
    try {
      const result = await partialStage(change.path, source === "staged", selection);
      dispatchSession({ type: "statusReady", status: result.status });
      return result;
    } catch (cause) {
      dispatchSession({ type: "statusError", message: "Changes may have changed. Refresh to continue." });
      throw cause;
    } finally { busyRef.current = false; setBusy(null); }
  }

  async function commit() {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return;
    if (!commitMessage.trim()) {
      setCommitError("Enter a commit message.");
      return;
    }
    if (status.phase !== "ready") {
      setCommitError("Refresh changes before committing.");
      return;
    }
    if (status.data.staged.length === 0) {
      setCommitError("Stage at least one change before committing.");
      return;
    }
    busyRef.current = true;
    setBusy("Committing staged changes");
    setSelectedChange(null);
    setCommitError("");
    setCommitNotice("");
    try {
      const result = await commitStaged(commitMessage);
      branchRequest.current += 1;
      dispatchSession({ type: "commit", result });
      if (branches.phase !== "ready") void loadBranches();
      setHistoryVersion((value) => value + 1);
      setCommitMessage("");
      setCommitNotice(`Committed ${result.head.hash.slice(0, 10)}.`);
    } catch (cause) {
      const message = messageForError(cause);
      setCommitError(message);
      dispatchSession({ type: "statusError", message: "Changes may have changed. Refresh to continue." });
      if (errorCode(cause) === "commit_refresh_failed") {
        branchRequest.current += 1;
        dispatchSession({ type: "reset" });
        setOpenError(message);
      }
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function createLocalBranch(name: string): Promise<boolean> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return false;
    busyRef.current = true;
    setBusy(`Creating ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      dispatchSession({ type: "branchesReady", branches: await createBranch(name) });
      return true;
    } catch (cause) {
      setBranchError(messageForError(cause));
      if (errorCode(cause) === "branch_refresh_failed") dispatchSession({ type: "branchesError", message: "Branch list is out of date. Retry to refresh." });
      return false;
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function switchBranch(name: string) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return;
    busyRef.current = true;
    setBusy(`Switching to ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const result = await checkoutBranch(name);
      dispatchSession({ type: "checkout", result });
      setSelectedChange(null);
      setHistoryVersion((value) => value + 1);
      setCommitNotice("");
      setCommitError("");
    } catch (cause) {
      const message = messageForError(cause);
      setBranchError(message);
      if (errorCode(cause) === "checkout_refresh_failed") {
        branchRequest.current += 1;
        dispatchSession({ type: "reset" });
        setSelectedChange(null);
        setOpenError("Branch switched, but its updated state could not be loaded. Reopen the repository.");
      }
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  async function changeBranchRef(label: string, operation: () => Promise<import("./lib/repository").BranchRefMutationResult>): Promise<boolean> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return false;
    busyRef.current = true;
    setBusy(label);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const result = await operation();
      dispatchSession({ type: "branchRefs", result });
      setHistoryRefreshVersion((value) => value + 1);
      return true;
    } catch (cause) {
      setBranchError(messageForError(cause));
      if (errorCode(cause) === "branch_refresh_failed") inconsistentManagement("Branch refs changed, but updated state could not be loaded.");
      return false;
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  }

  function applyStashMutation(result: StashMutationResult) {
    dispatchSession({ type: "stash", result });
    if (result.conflicts) setConflictVersion((value) => value + 1);
    if (result.clearDiff) setSelectedChange(null);
    if (result.resetHistory) setHistoryVersion((value) => value + 1);
  }

  function inconsistentManagement(message: string) {
    branchRequest.current += 1;
    dispatchSession({ type: "reset" });
    setSelectedChange(null);
    setOpenError(`${message} Reopen the repository.`);
  }

  function managementBusyChanged(value: boolean) {
    managementBusyRef.current = Math.max(0, managementBusyRef.current + (value ? 1 : -1));
    setManagementBusy(managementBusyRef.current > 0);
  }

  function applyRemoteRefresh(result: RemoteRefresh) {
    branchRequest.current += 1;
    dispatchSession({ type: "remote", result });
    if (result.conflicts) setConflictVersion((value) => value + 1);
    if (result.clearDiff) setSelectedChange(null);
    if (result.resetHistory) setHistoryVersion((value) => value + 1);
    else if (result.refreshHistory) setHistoryRefreshVersion((value) => value + 1);
    if (result.refreshHistory) setTagRefreshVersion((value) => value + 1);
  }

  function applyMergeResult(result: MergeMutationResult) {
    branchRequest.current += 1;
    dispatchSession({ type: "historyMutation", result });
    setConflictVersion((value) => value + 1);
    if (result.clearDiff) setSelectedChange(null);
    if (result.resetHistory) setHistoryVersion((value) => value + 1);
    setCommitNotice("");
    setCommitError("");
  }

  function applyHistoryResult(result: HistoryMutationResult) {
    branchRequest.current += 1;
    dispatchSession({ type: "historyMutation", result });
    setConflictVersion((value) => value + 1);
    if (result.clearDiff) setSelectedChange(null);
    if (result.resetHistory) setHistoryVersion((value) => value + 1);
    setCommitNotice(""); setCommitError("");
  }

  async function runSelectedCommitAction(action: "cherry_pick" | "revert" | "reset", commit: string, mode?: ResetMode): Promise<void> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || operation.kind !== "none" || conflicts.length > 0) return;
    busyRef.current = true;
    setHistoryActionError(""); setHistoryActionNotice("");
    setBusy(action === "reset" ? `Resetting (${mode})` : action === "revert" ? "Reverting commit" : "Cherry-picking commit");
    try {
      const result = action === "cherry_pick" ? await cherryPick(commit) : action === "revert" ? await revertCommit(commit) : await resetTo(commit, mode!);
      applyHistoryResult(result);
      if (result.error) throw result.error;
      setHistoryActionNotice(result.state.operation.kind === "none" ? `${action === "reset" ? "Reset" : action === "revert" ? "Revert" : "Cherry-pick"} complete.` : "Operation stopped. Resolve and stage conflicts above.");
    } catch (cause) {
      setHistoryActionError(messageForError(cause));
      if (errorCode(cause) === "merge_refresh_failed") inconsistentManagement("Git ran, but updated state could not be loaded.");
      throw cause;
    } finally { busyRef.current = false; setBusy(null); }
  }

  async function tagSelectedCommit(hash: string): Promise<void> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || operation.kind !== "none") return;
    const name = window.prompt(`Create lightweight tag at ${hash.slice(0, 10)}:`, "");
    if (!name?.trim()) return;
    busyRef.current = true;
    setBusy(`Creating tag ${name.trim()}`);
    setHistoryActionError("");
    try {
      await createTag(name.trim(), null, hash);
      setTagRefreshVersion((value) => value + 1);
      setHistoryRefreshVersion((value) => value + 1);
      setHistoryActionNotice(`Created tag ${name.trim()}.`);
    } catch (cause) {
      setHistoryActionError(messageForError(cause));
      if (errorCode(cause) === "tag_refresh_failed") inconsistentManagement("Tag refs changed, but updated state could not be loaded.");
      throw cause;
    } finally { busyRef.current = false; setBusy(null); }
  }

  async function branchAtHead(hash: string): Promise<void> {
    if (repositoryRef.current?.head?.hash !== hash || busyRef.current || managementBusyRef.current > 0 || remoteBusy || operation.kind !== "none") throw new Error("The selected commit is no longer the current HEAD.");
    const name = window.prompt(`Create local branch at ${hash.slice(0, 10)}:`, "");
    if (!name?.trim()) return;
    busyRef.current = true;
    setBusy(`Creating ${name.trim()}`);
    try {
      dispatchSession({ type: "branchesReady", branches: await createBranch(name.trim()) });
      setHistoryRefreshVersion((value) => value + 1);
      setHistoryActionNotice(`Created branch ${name.trim()}.`);
    } catch (cause) { setHistoryActionError(messageForError(cause)); if (errorCode(cause) === "branch_refresh_failed") inconsistentManagement("Branch refs changed, but updated state could not be loaded."); throw cause; }
    finally { busyRef.current = false; setBusy(null); }
  }

  async function rebaseOntoSelectedBranch(fullRef: string): Promise<void> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || operation.kind !== "none" || conflicts.length > 0) return;
    busyRef.current = true;
    setBusy("Rebasing current branch");
    setHistoryActionError("");
    try {
      const result = await rebaseOnto(fullRef);
      applyHistoryResult(result);
      if (result.error) throw result.error;
      setHistoryActionNotice(result.state.operation.kind === "none" ? "Rebase complete." : "Rebase stopped. Resolve and stage conflicts in Merge / Conflicts.");
    } catch (cause) { setHistoryActionError(messageForError(cause)); if (errorCode(cause) === "merge_refresh_failed") inconsistentManagement("Git ran, but updated state could not be loaded."); throw cause; }
    finally { busyRef.current = false; setBusy(null); }
  }

  function applyConflictResult(result: ConflictMutationResult) {
    applyRepositoryState(result.state);
    setSelectedChange(null);
  }

  const changes = status.phase === "ready" ? status.data : null;
  const mergeActive = operation.kind !== "none";
  const repositoryMutationDisabled = mergeActive || conflicts.length > 0;
  const hasStageable = !!changes && changes.unstaged.length + changes.untracked.length + changes.conflicted.length > 0;
  const hasStaged = !!changes && changes.staged.length > 0;
  const controlsBusy = !!busy || remoteBusy || managementBusy;
  const changeCount = changes ? changes.staged.length + changes.unstaged.length + changes.untracked.length + changes.conflicted.length : 0;
  const activeWorkspace = workspaceCatalog?.workspaces.find((item) => item.id === workspaceCatalog.activeWorkspaceId);

  function showFileMenu(source: "staged" | "unstaged" | "untracked", change: FileChange, event: React.MouseEvent) {
    event.preventDefault();
    setFileMenu({ x: event.clientX, y: event.clientY, source, change });
  }

  function fullFilePath(file: FileChange): string {
    const base = repository?.path.replace(/[\\/]$/, "") ?? "";
    const separator = base.includes("\\") ? "\\" : "/";
    return `${base}${separator}${file.path.replaceAll("/", separator)}`;
  }

  useEffect(() => {
    if (repository && operation.kind !== "none") { setShowOpen(false); setActiveView("merge"); }
  }, [repository, operation.kind]);

  return <WorkspaceLayout repository={repository} repositoryError={status.phase === "error" ? status.message : undefined} activeView={activeView} onViewChange={(view) => { setShowOpen(false); setActiveView(view); }} onOpen={() => setShowOpen(true)} openDisabled={controlsBusy} changeCount={changeCount} busy={busy}
    workspaceName={activeWorkspace?.name ?? "Default"}
    workspaceMenu={<WorkspacePanel catalog={workspaceCatalog} busy={workspaceBusy || controlsBusy} error={workspaceError}
      onCreate={(name) => void runWorkspaceTransition("Creating workspace", () => createWorkspace(name))}
      onRename={(id, name) => void renameCurrentWorkspace(id, name)}
      onDelete={(id) => void runWorkspaceTransition("Deleting workspace", () => deleteWorkspace(id))}
      onSwitch={(id) => void runWorkspaceTransition("Switching workspace", () => switchWorkspace(id))}
      onSelectRepository={(workspaceId, repositoryId) => void runWorkspaceTransition("Switching repository", () => switchWorkspaceRepository(workspaceId, repositoryId))}
      onRemoveRepository={(workspaceId, repositoryId) => void runWorkspaceTransition("Removing repository", () => removeRepository(workspaceId, repositoryId))} />}
    repositoryTabs={activeWorkspace?.repositories.map((item) => <button type="button" key={item.id} className={`woo-repo-tab ${activeWorkspace.activeRepositoryId === item.id ? "active" : ""}`} title={item.path} disabled={controlsBusy || workspaceBusy} onClick={() => void runWorkspaceTransition("Switching repository", () => switchWorkspaceRepository(activeWorkspace.id, item.id))}>{activeWorkspace.activeRepositoryId === item.id && changeCount > 0 && <span className="woo-dirty">●</span>}{item.path.split(/[\\/]/).filter(Boolean).at(-1) ?? item.path}</button>)}
    activity={(open) => <OperationHistory active={open} />}
    remoteControls={repository && <RemotePanel key={`${repository.path}:${sessionVersion}`} onComplete={applyRemoteRefresh} onInconsistent={inconsistentManagement} onBusyChange={setRemoteBusy} localBusy={!!busy || managementBusy || repositoryMutationDisabled} />}
    sidebar={repository && <BranchPanel key={`sidebar:${repository.path}:${sessionVersion}`} compact state={branches} currentBranch={repository.branch} busy={controlsBusy || repositoryMutationDisabled} onCreate={createLocalBranch} onCheckout={(name) => void switchBranch(name)} onRename={(ref, name) => changeBranchRef(`Renaming ${ref}`, () => renameBranch(ref, name))} onDelete={(ref) => changeBranchRef(`Deleting ${ref}`, () => deleteBranch(ref))} onRetry={() => void loadBranches()} error={branchError} />}>
    {(!repository || showOpen) && <div className="woo-open-view">
        <div className="intro"><p className="eyebrow">YOUR WORKSPACE</p><h1>Workspaces and repositories</h1><p>Choose one active repository. Git state loads only for that repository.</p></div>
        <div className="open-panel">
          <label htmlFor="repo-path">Add repository to active workspace</label>
        <div className="path-controls">
          <input id="repo-path" value={path} onChange={(event) => setPath(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void loadRepository(path); }} placeholder="C:\\path\\to\\repository" disabled={!!busy} />
          <button className="secondary" onClick={() => void chooseRepository()} disabled={!!busy}>Browse…</button>
          <button onClick={() => void loadRepository(path)} disabled={!!busy || !path.trim() || !workspaceCatalog?.activeWorkspaceId}>{busy === "Adding repository" ? "Adding…" : "Add and open"}</button>
        </div>
        {openError && <p className="error" role="alert">{openError}</p>}
      </div>
      {repository && <button className="secondary woo-open-close" onClick={() => setShowOpen(false)}>Return to workspace</button>}
    </div>}
      {repository && <>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "branches"}>
        <section className="repo-card" aria-label="Repository information">
          <div className="repo-heading"><span className="repo-icon">⌘</span><div><p className="eyebrow">REPOSITORY</p><h2>{repository.path.split(/[\\/]/).filter(Boolean).at(-1)}</h2></div></div>
          <dl>
            <div><dt>Path</dt><dd className="path-value">{repository.path}</dd></div>
            <div><dt>Current branch</dt><dd>{repository.branch ?? "Detached HEAD"}</dd></div>
            <div><dt>HEAD</dt><dd>{repository.head ? <><code>{repository.head.hash.slice(0, 10)}</code><span className="subject">{repository.head.subject}</span></> : "No commits yet"}</dd></div>
          </dl>
        </section>
        <BranchPanel key={`tool:${repository.path}:${sessionVersion}`} state={branches} currentBranch={repository.branch} busy={controlsBusy || repositoryMutationDisabled} onCreate={createLocalBranch} onCheckout={(name) => void switchBranch(name)} onRename={(ref, name) => changeBranchRef(`Renaming ${ref}`, () => renameBranch(ref, name))} onDelete={(ref) => changeBranchRef(`Deleting ${ref}`, () => deleteBranch(ref))} onRetry={() => void loadBranches()} error={branchError} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "merge"}>
        <MergePanel key={`merge:${repository.path}:${sessionVersion}`} branches={branches.phase === "ready" ? branches.data.branches : []} operation={operation} conflicts={conflicts} busy={controlsBusy || status.phase !== "ready"} refreshToken={conflictVersion} onBusyChange={managementBusyChanged} onMerge={applyMergeResult} onHistory={applyHistoryResult} onConflict={applyConflictResult} onInconsistent={inconsistentManagement} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "stashes"}>
        <StashPanel key={`stash:${repository.path}:${sessionVersion}`} busy={controlsBusy || repositoryMutationDisabled} onBusyChange={managementBusyChanged} onMutation={applyStashMutation} onInconsistent={inconsistentManagement} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "tags"}>
        <TagPanel key={`tag:${repository.path}:${sessionVersion}`} refreshToken={tagRefreshVersion} busy={controlsBusy || repositoryMutationDisabled} onBusyChange={managementBusyChanged} onMutation={() => setHistoryRefreshVersion((value) => value + 1)} onInconsistent={inconsistentManagement} />
        </div>
        <section className="changes-panel woo-view" aria-label="Working tree changes" hidden={showOpen || activeView !== "changes"}>
          <div className="woo-changes-list">
          <div className="changes-heading"><div><p className="eyebrow">WORKING TREE</p><h2>Changes</h2></div><FileViewToggle mode={localFilesMode} onChange={setLocalFilesMode} /><div className="change-actions">
            <button className="secondary" disabled={controlsBusy} onClick={() => void refresh()}>Refresh</button>
            <button className="secondary" disabled={controlsBusy || repositoryMutationDisabled || !hasStaged} onClick={() => void changeIndex("Unstaging all", unstageAll)}>Unstage All</button>
            <button disabled={controlsBusy || repositoryMutationDisabled || !hasStageable} onClick={() => void changeIndex("Staging all", stageAll)}>Stage All</button>
          </div></div>
          {busy && <p className="operation-feedback" role="status">{busy}…</p>}
          {status.phase === "loading" && <p className="status-placeholder">Loading changes…</p>}
          {status.phase === "error" && <p className="error" role="alert">{status.message}</p>}
          {changes && <div className="change-groups">
            <FileGroup title="Staged" files={changes.staged} mode={localFilesMode} selectedPath={selectedChange?.source === "staged" ? selectedChange.change.path : null} action="Unstage" onSelect={(file) => setSelectedChange({ source: "staged", change: file })} onContextMenu={(file, event) => showFileMenu("staged", file, event)} onAction={(file) => void changeIndex(`Unstaging ${file.path}`, () => unstageFile(file))} busy={controlsBusy || repositoryMutationDisabled} />
            <FileGroup title="Unstaged" files={changes.unstaged} mode={localFilesMode} selectedPath={selectedChange?.source === "unstaged" ? selectedChange.change.path : null} action="Stage" onSelect={(file) => setSelectedChange({ source: "unstaged", change: file })} onContextMenu={(file, event) => showFileMenu("unstaged", file, event)} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(file))} busy={controlsBusy || repositoryMutationDisabled} />
            <FileGroup title="Untracked" files={changes.untracked} mode={localFilesMode} selectedPath={selectedChange?.source === "untracked" ? selectedChange.change.path : null} action="Stage" onSelect={(file) => setSelectedChange({ source: "untracked", change: file })} onContextMenu={(file, event) => showFileMenu("untracked", file, event)} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(file))} busy={controlsBusy || repositoryMutationDisabled} />
            {changes.conflicted.length > 0 && <FileGroup title="Conflicted" files={changes.conflicted} mode={localFilesMode} busy={controlsBusy || repositoryMutationDisabled} />}
          </div>}
          <div className="commit-area">
            <div><h3>Commit staged changes</h3><p>Only files in Staged are included in this commit.</p></div>
            <label htmlFor="commit-message">Commit message</label>
            <textarea id="commit-message" value={commitMessage} disabled={controlsBusy || repositoryMutationDisabled} rows={4} placeholder="Describe your change" onChange={(event) => { setCommitMessage(event.target.value); setCommitError(""); setCommitNotice(""); }} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void commit(); } }} />
            <div className="commit-footer"><span>{repositoryMutationDisabled ? "Finish the current repository operation above." : !hasStaged ? "Stage a change to enable commit." : "Ctrl+Enter to commit"}</span><button disabled={controlsBusy || repositoryMutationDisabled || !hasStaged || status.phase !== "ready"} onClick={() => void commit()}>{busy === "Committing staged changes" ? "Committing…" : "Commit"}</button></div>
            {commitError && <p className="error" role="alert">{commitError}</p>}
            {commitNotice && <p className="commit-notice" role="status">{commitNotice}</p>}
          </div>
          </div>
          <div className="woo-changes-detail">
            {selectedChange && changes ? <DiffViewer key={`${selectedChange.source}:${selectedChange.change.path}:${selectedChange.change.oldPath ?? ""}:${diffRefreshVersion}`} source={selectedChange.source} change={selectedChange.change} busy={controlsBusy || repositoryMutationDisabled} onPartial={selectedChange.source === "staged" || selectedChange.source === "unstaged" ? (selection) => changePart(selectedChange.source as "staged" | "unstaged", selectedChange.change, selection) : undefined} /> : <p className="woo-detail-empty">Select a changed file to view its diff.</p>}
          </div>
        </section>
        <div className="woo-view woo-history-view" hidden={showOpen || activeView !== "history"}>
          {historyActionError && <p className="error" role="alert">{historyActionError}</p>}{historyActionNotice && <p className="commit-notice" role="status">{historyActionNotice}</p>}
          <HistoryPanel key={`${repository.path}:${historyVersion}`} refreshToken={historyRefreshVersion} actionBusy={controlsBusy || repositoryMutationDisabled} onAction={runSelectedCommitAction} onTag={tagSelectedCommit} onBranch={branchAtHead} currentHead={repository.head?.hash} rebaseTargets={branches.phase === "ready" ? branches.data.branches.filter((branch) => branch.kind === "local" && !branch.isCurrent).map((branch) => ({ name: branch.fullRefName, hash: branch.targetHash })) : []} onRebase={rebaseOntoSelectedBranch} />
        </div>
      </>}
      {fileMenu && <ContextMenu x={fileMenu.x} y={fileMenu.y} title={fileMenu.change.path} onClose={() => setFileMenu(null)} actions={[
        { label: fileMenu.source === "staged" ? "Unstage" : "Stage", disabled: controlsBusy || repositoryMutationDisabled, onSelect: () => { const { source, change } = fileMenu; void changeIndex(`${source === "staged" ? "Unstaging" : "Staging"} ${change.path}`, () => source === "staged" ? unstageFile(change) : stageFile(change)); } },
        { label: fileMenu.source === "staged" ? "Unstage selected lines…" : "Stage selected lines…", disabled: fileMenu.source === "untracked" || controlsBusy || repositoryMutationDisabled, title: "Select changed lines in the diff pane", onSelect: () => setSelectedChange({ source: fileMenu.source, change: fileMenu.change }) },
        { label: "Discard changes…", disabled: true, title: "Woo does not currently expose a safe discard operation", danger: true, onSelect: () => {} },
        { label: "Open file", disabled: true, title: "External file opening is not yet available", onSelect: () => {} },
        { label: "Open containing folder", disabled: true, title: "External folder opening is not yet available", onSelect: () => {} },
        { label: "Copy path", onSelect: () => void (navigator.clipboard?.writeText(fullFilePath(fileMenu.change)) ?? Promise.reject()).catch(() => setCommitError("Could not copy the file path.")) },
      ]} />}
  </WorkspaceLayout>;
}
