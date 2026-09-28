import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import HistoryPanel, { type HistorySnapshot } from "./features/history/HistoryPanel";
import RemotePanel from "./features/remotes/RemotePanel";
import BranchPanel from "./features/branches/BranchPanel";
import StashPanel from "./features/stash/StashPanel";
import TagPanel from "./features/tags/TagPanel";
import MergePanel from "./features/operations/MergePanel";
import OperationHistory from "./features/operations/OperationHistory";
import DiffViewer, { type DiffSource } from "./features/diff/DiffViewer";
import { useRepositorySession } from "./app/repository-session/useRepositorySession";
import { RepositoryView } from "./app/repository-session/RepositoryView";
import { RepositoryCache, RepositoryPrewarmQueue, refsChanged, repositoryCacheKey, sameIdentity, sameRepositoryState } from "./app/repository-session/repositoryCache";
import { FileGroup } from "./features/changes/FileGroup";
import { FileViewToggle, type FileViewMode } from "./features/changes/FilePresentation";
import { ContextMenu } from "./components/ui/ContextMenu";
import { WorkspaceLayout, type WorkspaceView } from "./layout/WorkspaceLayout";
import { WorkspacePanel } from "./features/workspace/WorkspacePanel";
import { activateRepositoryWatch, createWorkspace, deleteWorkspace, registerRepository, removeRepository, renameWorkspace, restoreWorkspaceSession, selectWorkspaceRepository, switchWorkspace, type WorkspaceCatalog, type WorkspaceTransition } from "./features/workspace/workspace";
import {
  checkoutBranch, commitStaged, createBranch, deleteBranch, errorCode, getBranches, getCommitHistory, getRemotes, getRepositoryInfo, getRepositoryState, getStashes, getTags, messageForError, partialStage, renameBranch, revalidateRepository, stageAll, stageFile,
  cherryPick, createTag, rebaseOnto, resetTo, revertCommit, unstageAll, unstageFile, type AutoRefreshEvent, type ConflictMutationResult, type DiffFile, type FileChange, type HistoryMutationResult, type MergeMutationResult, type PartialSelection, type PartialStageResult, type RemoteRefresh, type RepositoryState, type RepositoryStatus, type ResetMode, type StashMutationResult,
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
  const [localDiff, setLocalDiff] = useState<DiffFile | null>(null);
  const [sessionReady, setSessionReady] = useState(true);
  const [refreshError, setRefreshError] = useState("");
  const [revalidating, setRevalidating] = useState(false);
  const [branchError, setBranchError] = useState("");
  const [historyVersion, setHistoryVersion] = useState(0);
  const [historyRefreshVersion, setHistoryRefreshVersion] = useState(0);
  const [tagRefreshVersion, setTagRefreshVersion] = useState(0);
  const [stashRefreshVersion, setStashRefreshVersion] = useState(0);
  const [diffRefreshVersion, setDiffRefreshVersion] = useState(0);
  const [historyActionError, setHistoryActionError] = useState("");
  const [historyActionNotice, setHistoryActionNotice] = useState("");
  const [remoteBusyById, setRemoteBusyById] = useState<Record<string, boolean>>({});
  const [managementBusyById, setManagementBusyById] = useState<Record<string, boolean>>({});
  const managementBusyRef = useRef(0);
  const managementCounts = useRef(new Map<string, number>());
  // Visited tabs stay mounted; this list is independent of the bounded data
  // cache and keyed by the canonical repository identity, not workspace ID.
  const [mountedViews, setMountedViews] = useState<string[]>([]);
  const branchRequest = useRef(0);
  const repositoryRequest = useRef(0);
  const restoreStarted = useRef(false);
  const busyRef = useRef(false);
  const repositoryRef = useRef(repository);
  const lastWatchSequence = useRef(0);
  const pendingWatch = useRef<AutoRefreshEvent | null>(null);
  const pendingBackgroundRefresh = useRef(false);
  const remoteBusyRef = useRef(false);
  const cache = useRef(new RepositoryCache());
  const refBaselines = useRef(new Map<string, { tags: Awaited<ReturnType<typeof getTags>>; stashes: Awaited<ReturnType<typeof getStashes>> }>());
  const activeCacheKey = useRef<string | null>(null);
  const validationEpoch = useRef(0);
  const prewarmQueue = useRef(new RepositoryPrewarmQueue());
  const prewarmRunning = useRef(false);
  const foregroundStatusReady = useRef(false);
  const foregroundHistoryReady = useRef(false);
  const selectionSave = useRef<Promise<unknown>>(Promise.resolve());
  const activeRepositoryIdRef = useRef<string | null>(null);
  const watchSessions = useRef(new Map<string, import("./lib/repository").RepositoryInfo>());
  const activeRepositoryId = workspaceCatalog?.workspaces.find((item) => item.id === workspaceCatalog.activeWorkspaceId)?.activeRepositoryId ?? null;
  const remoteBusy = !!(activeRepositoryId && remoteBusyById[activeRepositoryId]);
  const managementBusy = !!(activeRepositoryId && managementBusyById[activeRepositoryId]);
  managementBusyRef.current = activeRepositoryId ? managementCounts.current.get(activeRepositoryId) ?? 0 : 0;
  if (workspaceCatalog) activeRepositoryIdRef.current = activeRepositoryId;
  const repositoryIdForPath = (viewPath: string) => workspaceCatalog?.workspaces.find((item) => item.id === workspaceCatalog.activeWorkspaceId)?.repositories.find((item) => repositoryCacheKey(item.path) === repositoryCacheKey(viewPath))?.id ?? "";
  repositoryRef.current = repository;
  remoteBusyRef.current = remoteBusy;
  useEffect(() => { if (!selectedChange) setLocalDiff(null); }, [selectedChange]);

  function saveActiveSession() {
    if (!repository || !activeCacheKey.current) return;
    const old = cache.current.get(repository.path);
    cache.current.set(repository.path, {
      session, history: old?.history ?? null, activeView, localFilesMode, selectedChange, localDiff,
      commitMessage, conflictVersion, historyVersion, historyRefreshVersion, diffRefreshVersion, tagRefreshVersion, stashRefreshVersion,
      refs: { branches: branches.phase === "ready" ? branches.data : old?.refs.branches ?? null,
        tags: refBaselines.current.get(repositoryCacheKey(repository.path))?.tags ?? old?.refs.tags ?? null,
        stashes: refBaselines.current.get(repositoryCacheKey(repository.path))?.stashes ?? old?.refs.stashes ?? null,
        remotes: old?.refs.remotes ?? null },
      freshness: "stale", generation: (old?.generation ?? 0) + 1,
    });
    refBaselines.current.delete(repositoryCacheKey(repository.path));
  }

  function invalidateWarmValidation() {
    validationEpoch.current += 1;
    if (repositoryRef.current) cache.current.markStale(repositoryRef.current.path);
    setRevalidating(false);
  }

  function restoreCachedSession(path: string): boolean {
    const saved = cache.current.get(path);
    if (!saved || !saved.session.repository) return false;
    activeCacheKey.current = repositoryCacheKey(path);
    repositoryRef.current = saved.session.repository;
    dispatchSession({ type: "restore", session: saved.session });
    setActiveView(saved.activeView);
    setLocalFilesMode(saved.localFilesMode);
    setSelectedChange(saved.selectedChange);
    setLocalDiff(saved.localDiff);
    setCommitMessage(saved.commitMessage);
    setConflictVersion(saved.conflictVersion);
    setCommitError("");
    setCommitNotice("");
    setBranchError("");
    setHistoryActionError("");
    setHistoryActionNotice("");
    setFileMenu(null);
    setHistoryVersion(saved.historyVersion);
    setHistoryRefreshVersion(saved.historyRefreshVersion);
    setDiffRefreshVersion(saved.diffRefreshVersion);
    setTagRefreshVersion(saved.tagRefreshVersion);
    setStashRefreshVersion(saved.stashRefreshVersion);
    setShowOpen(false);
    setSessionReady(true);
    setRevalidating(false);
    setRefreshError("");
    return true;
  }

  function applyWatchEvent(event: AutoRefreshEvent) {
    const current = repositoryRef.current;
    if (!current || event.sessionId !== current.sessionId) {
      if (event.repositoryId && watchSessions.current.has(event.repositoryId)) cache.current.markStale(watchSessions.current.get(event.repositoryId)!.path);
      else for (const info of watchSessions.current.values()) if (info.sessionId === event.sessionId) cache.current.markStale(info.path);
      return;
    }
    if (event.sequence <= lastWatchSequence.current) return;
    lastWatchSequence.current = event.sequence;
    cache.current.markStale(current.path);
    setFileMenu(null);
    if (event.unavailable) {
      dispatchSession({ type: "statusError", message: event.unavailable.message, clearOperation: true });
      setSelectedChange(null);
      setLocalDiff(null);
      return;
    }
    if (event.state) {
      setRefreshError("");
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
    if (event.clearDiff || actualHeadChanged) { setSelectedChange(null); setLocalDiff(null); }
    else if (selectedChange) {
      const group = event.state?.status[selectedChange.source as "staged" | "unstaged" | "untracked"];
      if (group && !group.some((change) => change.path === selectedChange.change.path)) setSelectedChange(null);
      else if (event.diffPaths.some((path) => path === selectedChange.change.path || selectedChange.change.path.startsWith(`${path}/`))) {
        setDiffRefreshVersion((value) => value + 1);
        setLocalDiff(null);
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
    void listen<{ repositoryId: string | null; sessionId: number; operationId: number; refresh: RemoteRefresh | null }>("background-fetch-completed", ({ payload }) => {
      if (repositoryRef.current?.sessionId !== payload.sessionId) {
        if (payload.repositoryId && watchSessions.current.has(payload.repositoryId)) cache.current.markStale(watchSessions.current.get(payload.repositoryId)!.path);
        else for (const info of watchSessions.current.values()) if (info.sessionId === payload.sessionId) cache.current.markStale(info.path);
        return;
      }
      if (!payload.refresh) return;
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
    void revalidateRepository(activeRepositoryId!);
  }, [busy, managementBusy, remoteBusy]);

  function applyRepositoryState(state: RepositoryState) {
    setRefreshError("");
    dispatchSession({ type: "repositoryState", state });
    setFileMenu(null);
    setConflictVersion((value) => value + 1);
  }

  async function loadBranches(repositoryId = activeRepositoryId ?? "") {
    const request = ++branchRequest.current;
    setBranchError("");
    dispatchSession({ type: "branchesLoading" });
    try {
      const data = await getBranches(repositoryId);
      if (request === branchRequest.current) dispatchSession({ type: "branchesReady", branches: data });
    } catch (cause) {
      if (request === branchRequest.current) dispatchSession({ type: "branchesError", message: messageForError(cause) });
    }
  }

  function resetRepositorySession() {
    repositoryRequest.current += 1;
    validationEpoch.current += 1;
    activeCacheKey.current = null;
    setPath("");
    repositoryRef.current = null;
    dispatchSession({ type: "reset" });
    lastWatchSequence.current = 0;
    pendingWatch.current = null;
    pendingBackgroundRefresh.current = false;
    branchRequest.current += 1;
    setBranchError("");
    setSelectedChange(null);
    setLocalDiff(null);
    setFileMenu(null);
    setCommitMessage("");
    setCommitError("");
    setCommitNotice("");
    setHistoryActionError(""); setHistoryActionNotice("");
    setSessionReady(false);
  }

  async function attachRepository(info: NonNullable<WorkspaceTransition["repository"]>, repositoryId: string) {
    info = watchSessions.current.get(repositoryId) ?? info;
    const request = repositoryRequest.current;
    const saved = cache.current.get(info.path);
    const warm = !!saved?.session.repository;
    if (warm && saved?.session.status.phase === "ready") foregroundStatusReady.current = true;
    const identityChanged = warm && !sameIdentity(saved!.session.repository, info);
    if (warm && activeCacheKey.current !== repositoryCacheKey(info.path)) restoreCachedSession(info.path);
    activeCacheKey.current = repositoryCacheKey(info.path);
    cache.current.retainView(info.path);
    setMountedViews((current) => current.some((path) => repositoryCacheKey(path) === repositoryCacheKey(info.path)) ? current : [...current, info.path]);
    repositoryRef.current = info;
    dispatchSession({ type: "opened", repository: info });
    setShowOpen(false);
    if (!warm) setActiveView("history");
    if (!warm) setHistoryVersion((value) => value + 1);
    setPath(info.path);
    setSessionReady(true);
    if (identityChanged) {
      setHistoryVersion((value) => value + 1);
      setSelectedChange(null);
      setLocalDiff(null);
    }
    if (!warm) { dispatchSession({ type: "statusLoading" }); void loadBranches(repositoryId); }
    const statusSequence = lastWatchSequence.current;
    if (warm) {
      const generation = cache.current.beginRefresh(info.path);
      const validation = ++validationEpoch.current;
      setRevalidating(true);
      setRefreshError("");
      void Promise.all([getRepositoryState(repositoryId), getBranches(repositoryId), getTags(repositoryId), getStashes(repositoryId)]).then(([state, nextBranches, tags, stashes]) => {
        if (request !== repositoryRequest.current || validation !== validationEpoch.current) return;
        if (statusSequence !== lastWatchSequence.current) { setRevalidating(false); return; }
        if (!identityChanged && refsChanged(saved!.refs, { branches: nextBranches, tags, stashes })) {
          setHistoryRefreshVersion((value) => value + 1);
          setTagRefreshVersion((value) => value + 1);
        }
        if (!saved!.refs.stashes || JSON.stringify(saved!.refs.stashes) !== JSON.stringify(stashes)) setStashRefreshVersion((value) => value + 1);
        if (saved!.session.status.phase !== "ready" || !sameRepositoryState({ status: saved!.session.status.data, operation: saved!.session.operation, conflicts: saved!.session.conflicts }, state)) {
          applyRepositoryState(state);
        }
        // Status records paths/kinds, not file content. The inactive watcher was
        // stopped, so a changed file can have an identical status record.
        if (saved!.selectedChange) {
          const group = state.status[saved!.selectedChange.source as "staged" | "unstaged" | "untracked"];
          if (!group.some((change) => change.path === saved!.selectedChange!.change.path)) setSelectedChange(null);
          else setDiffRefreshVersion((value) => value + 1);
          setLocalDiff(null);
        }
        dispatchSession({ type: "branchesReady", branches: nextBranches });
        if (generation !== null) cache.current.finishRefresh(info.path, generation, (entry) => ({ ...entry, freshness: "fresh", refs: { branches: nextBranches, tags, stashes } }));
        setRevalidating(false);
      }).catch((cause) => {
        if (request !== repositoryRequest.current || validation !== validationEpoch.current) return;
        if (generation !== null) cache.current.finishRefresh(info.path, generation, (entry) => ({ ...entry, freshness: "stale" }));
        setRefreshError(`Could not refresh this repository: ${messageForError(cause)}`);
        setRevalidating(false);
      });
      return;
    }
    try {
      const initialState = await getRepositoryState(repositoryId);
      if (request === repositoryRequest.current && statusSequence === lastWatchSequence.current) {
        applyRepositoryState(initialState);
        foregroundStatusReady.current = true;
        if (foregroundHistoryReady.current) { prewarmQueue.current.releaseForeground(); void processPrewarmQueue(); }
      }
    } catch (cause) {
      if (request === repositoryRequest.current && statusSequence === lastWatchSequence.current) dispatchSession({ type: "statusError", message: messageForError(cause) });
    }
    void Promise.all([getTags(repositoryId), getStashes(repositoryId)]).then(([tags, stashes]) => {
      if (request === repositoryRequest.current) {
        if (cache.current.get(info.path)) cache.current.update(info.path, (entry) => ({ ...entry, refs: { ...entry.refs, tags, stashes } }));
        else refBaselines.current.set(repositoryCacheKey(info.path), { tags, stashes });
      }
    }).catch(() => {});
  }

  async function warmRepository(repositoryId: string, repositoryPath: string) {
    if (!cache.current.beginWarm(repositoryPath)) return;
    try {
      const info = await getRepositoryInfo(repositoryId);
      const state = await getRepositoryState(repositoryId);
      const nextBranches = await getBranches(repositoryId);
      const remotes = await getRemotes(repositoryId);
      const previous = cache.current.get(repositoryPath);
      const view = previous?.activeView ?? "history";
      let history: HistorySnapshot | null = previous?.history ?? null;
      if (view === "history" && !history?.pageLoaded) {
        const page = await getCommitHistory(repositoryId);
        history = {
          history: { commits: page.commits, graphRows: page.graphRows }, pageLoaded: true,
          maxLanes: Math.max(1, ...page.graphRows.map((row) => row.laneCount)),
          cursor: page.nextCursor, hasMore: page.hasMore, selectedHash: null,
          selectedSnapshot: null, commitFiles: [], filesLoaded: false,
          selectedFile: null, selectedDiff: null, commitFilesMode: "list", scrollTop: 0,
        };
      }
      if (activeRepositoryIdRef.current === repositoryId) {
        cache.current.failWarm(repositoryPath);
        return;
      }
      // A foreground activation may have populated this entry while Git was
      // loading. Keep that newer presentation and only fill missing warm data.
      const current = cache.current.get(repositoryPath);
      if (current?.session.repository) {
        cache.current.set(repositoryPath, { ...current, history: current.history?.pageLoaded ? current.history : history,
          refs: { ...current.refs, remotes }, freshness: current.freshness });
        return;
      }
      cache.current.set(repositoryPath, {
        session: { repository: info, status: { phase: "ready", data: state.status },
          branches: { phase: "ready", data: nextBranches }, operation: state.operation, conflicts: state.conflicts },
        history, activeView: view, localFilesMode: "list", selectedChange: null, localDiff: null,
        commitMessage: "", conflictVersion: 0, historyVersion: 0, historyRefreshVersion: 0,
        diffRefreshVersion: 0, tagRefreshVersion: 0, stashRefreshVersion: 0,
        refs: { branches: nextBranches, tags: null, stashes: null, remotes }, freshness: "fresh", generation: 0,
      });
    } catch {
      cache.current.failWarm(repositoryPath);
    }
  }

  async function processPrewarmQueue() {
    if (prewarmRunning.current) return;
    prewarmRunning.current = true;
    try {
      for (let next = prewarmQueue.current.nextBackground(); next; next = prewarmQueue.current.nextBackground()) {
        if (cache.current.warmState(next.path) === "cold") await warmRepository(next.id, next.path);
      }
    } finally { prewarmRunning.current = false; }
  }

  async function runWorkspaceTransition(label: string, action: () => Promise<WorkspaceTransition>, next?: { workspaceId: string; repositoryId: string; path: string } | null) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy) return;
    const previousCatalog = workspaceCatalog;
    if (label === "Restoring workspace") { foregroundStatusReady.current = false; foregroundHistoryReady.current = false; }
    const previousPath = repository?.path;
    saveActiveSession();
    repositoryRequest.current += 1;
    let request = repositoryRequest.current;
    validationEpoch.current += 1;
    branchRequest.current += 1;
    setRevalidating(false);
    lastWatchSequence.current = 0;
    pendingWatch.current = null;
    pendingBackgroundRefresh.current = false;
    busyRef.current = true;
    setWorkspaceBusy(true);
    setBusy(label);
    setWorkspaceError("");
    setOpenError("");
    const alreadyActive = next && workspaceCatalog?.activeWorkspaceId === next.workspaceId
      && workspaceCatalog.workspaces.find((item) => item.id === next.workspaceId)?.activeRepositoryId === next.repositoryId;
    if (next && !alreadyActive && restoreCachedSession(next.path) && workspaceCatalog) {
      setWorkspaceCatalog({ ...workspaceCatalog, activeWorkspaceId: next.workspaceId,
        workspaces: workspaceCatalog.workspaces.map((item) => item.id === next.workspaceId ? { ...item, activeRepositoryId: next.repositoryId } : item) });
    }
    try {
      const result = await action();
      if (request !== repositoryRequest.current) return;
      setWorkspaceCatalog(result.catalog);
      cache.current.prune(result.catalog);
      const registeredPaths = new Set(result.catalog.workspaces.flatMap((workspace) => workspace.repositories.map((item) => repositoryCacheKey(item.path))));
      setMountedViews((current) => current.filter((path) => registeredPaths.has(repositoryCacheKey(path))));
      for (const key of refBaselines.current.keys()) if (!registeredPaths.has(key)) refBaselines.current.delete(key);
      if (result.sessionChanged) {
        if (!result.repository || !cache.current.get(result.repository.path)) {
          resetRepositorySession();
          request = repositoryRequest.current;
        }
        if (result.repository) {
          const workspace = result.catalog.workspaces.find((item) => item.id === result.catalog.activeWorkspaceId);
          if (workspace?.activeRepositoryId) watchSessions.current.set(workspace.activeRepositoryId, result.repository);
          await attachRepository(result.repository, workspace?.activeRepositoryId ?? "");
          if (label === "Restoring workspace" && workspace) {
            for (const item of workspace.repositories) cache.current.retainView(item.path);
            prewarmQueue.current.restore(workspace.repositories, workspace.activeRepositoryId);
            if (foregroundStatusReady.current && (foregroundHistoryReady.current || cache.current.get(result.repository.path)?.activeView === "changes")) prewarmQueue.current.releaseForeground();
            void processPrewarmQueue();
          }
        }
        else { setShowOpen(true); if (result.openError) setOpenError(result.openError.message); }
      } else { setSessionReady(true); if (result.openError) setOpenError(result.openError.message); }
    } catch (cause) {
      if (request !== repositoryRequest.current) return;
      setWorkspaceError(messageForError(cause));
      if (previousCatalog) setWorkspaceCatalog(previousCatalog);
      if (previousPath) restoreCachedSession(previousPath);
      setSessionReady(true);
      setRevalidating(false);
    } finally {
      if (request === repositoryRequest.current) { busyRef.current = false; setWorkspaceBusy(false); setBusy(null); }
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
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy("Refreshing changes");
    dispatchSession({ type: "statusLoading" });
    setSelectedChange(null);
    setFileMenu(null);
    try {
      const next = await getRepositoryState(activeRepositoryId!);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      applyRepositoryState(next);
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      dispatchSession({ type: "statusError", message: messageForError(cause), clearOperation: true });
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
    }
  }

  async function changeIndex(label: string, operation: () => Promise<RepositoryStatus>) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(label);
    setSelectedChange(null);
    setCommitNotice("");
    try {
      const next = await operation();
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      dispatchSession({ type: "statusReady", status: next });
      setCommitError("");
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      dispatchSession({ type: "statusError", message: messageForError(cause) });
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
    }
  }

  async function changePart(source: "staged" | "unstaged", change: FileChange, selection: PartialSelection): Promise<PartialStageResult> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) throw new Error("Another repository operation is running.");
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(source === "staged" ? "Unstaging selected changes" : "Staging selected changes");
    try {
      const result = await partialStage(activeRepositoryId!, change.path, source === "staged", selection);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return result; }
      dispatchSession({ type: "statusReady", status: result.status });
      return result;
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); throw cause; }
      dispatchSession({ type: "statusError", message: "Changes may have changed. Refresh to continue." });
      throw cause;
    } finally { if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); } }
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
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy("Committing staged changes");
    setSelectedChange(null);
    setCommitError("");
    setCommitNotice("");
    try {
      const result = await commitStaged(activeRepositoryId!, commitMessage);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      branchRequest.current += 1;
      dispatchSession({ type: "commit", result });
      if (branches.phase !== "ready") void loadBranches();
      setHistoryVersion((value) => value + 1);
      setCommitMessage("");
      setCommitNotice(`Committed ${result.head.hash.slice(0, 10)}.`);
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      const message = messageForError(cause);
      setCommitError(message);
      dispatchSession({ type: "statusError", message: "Changes may have changed. Refresh to continue." });
      if (errorCode(cause) === "commit_refresh_failed") {
        branchRequest.current += 1;
        dispatchSession({ type: "reset" });
        setOpenError(message);
      }
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
    }
  }

  async function createLocalBranch(name: string): Promise<boolean> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return false;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(`Creating ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const next = await createBranch(activeRepositoryId!, name);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return true; }
      dispatchSession({ type: "branchesReady", branches: next });
      return true;
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return false; }
      setBranchError(messageForError(cause));
      if (errorCode(cause) === "branch_refresh_failed") dispatchSession({ type: "branchesError", message: "Branch list is out of date. Retry to refresh." });
      return false;
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
    }
  }

  async function switchBranch(name: string) {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(`Switching to ${name}`);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const result = await checkoutBranch(activeRepositoryId!, name);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      dispatchSession({ type: "checkout", result });
      setSelectedChange(null);
      setHistoryVersion((value) => value + 1);
      setCommitNotice("");
      setCommitError("");
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      const message = messageForError(cause);
      setBranchError(message);
      if (errorCode(cause) === "checkout_refresh_failed") {
        branchRequest.current += 1;
        dispatchSession({ type: "reset" });
        setSelectedChange(null);
        setOpenError("Branch switched, but its updated state could not be loaded. Reopen the repository.");
      }
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
    }
  }

  async function changeBranchRef(label: string, operation: () => Promise<import("./lib/repository").BranchRefMutationResult>): Promise<boolean> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || mergeActive || conflicts.length > 0) return false;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(label);
    setBranchError("");
    branchRequest.current += 1;
    try {
      const result = await operation();
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return true; }
      dispatchSession({ type: "branchRefs", result });
      setHistoryRefreshVersion((value) => value + 1);
      return true;
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return false; }
      setBranchError(messageForError(cause));
      if (errorCode(cause) === "branch_refresh_failed") inconsistentManagement("Branch refs changed, but updated state could not be loaded.");
      return false;
    } finally {
      if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); }
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

  function managementBusyChanged(repositoryId: string, value: boolean) {
    if (value && activeRepositoryIdRef.current === repositoryId) invalidateWarmValidation();
    const count = Math.max(0, (managementCounts.current.get(repositoryId) ?? 0) + (value ? 1 : -1));
    managementCounts.current.set(repositoryId, count);
    if (activeRepositoryIdRef.current === repositoryId) managementBusyRef.current = count;
    setManagementBusyById((current) => ({ ...current, [repositoryId]: count > 0 }));
  }

  function remoteBusyChanged(repositoryId: string, value: boolean) {
    if (value && activeRepositoryIdRef.current === repositoryId) invalidateWarmValidation();
    setRemoteBusyById((current) => ({ ...current, [repositoryId]: value }));
  }

  function applyRemoteRefreshFor(repositoryId: string, repositoryPath: string, result: RemoteRefresh) {
    if (activeRepositoryIdRef.current !== repositoryId) {
      cache.current.markStale(repositoryPath);
      return;
    }
    applyRemoteRefresh(result);
  }

  function applyRemoteRefresh(result: RemoteRefresh) {
    invalidateWarmValidation();
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
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setHistoryActionError(""); setHistoryActionNotice("");
    setBusy(action === "reset" ? `Resetting (${mode})` : action === "revert" ? "Reverting commit" : "Cherry-picking commit");
    try {
      const result = action === "cherry_pick" ? await cherryPick(activeRepositoryId!, commit) : action === "revert" ? await revertCommit(activeRepositoryId!, commit) : await resetTo(activeRepositoryId!, commit, mode!);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      applyHistoryResult(result);
      if (result.error) throw result.error;
      setHistoryActionNotice(result.state.operation.kind === "none" ? `${action === "reset" ? "Reset" : action === "revert" ? "Revert" : "Cherry-pick"} complete.` : "Operation stopped. Resolve and stage conflicts above.");
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); throw cause; }
      setHistoryActionError(messageForError(cause));
      if (errorCode(cause) === "merge_refresh_failed") inconsistentManagement("Git ran, but updated state could not be loaded.");
      throw cause;
    } finally { if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); } }
  }

  async function tagSelectedCommit(hash: string): Promise<void> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || operation.kind !== "none") return;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    const name = window.prompt(`Create lightweight tag at ${hash.slice(0, 10)}:`, "");
    if (!name?.trim()) return;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(`Creating tag ${name.trim()}`);
    setHistoryActionError("");
    try {
      await createTag(activeRepositoryId!, name.trim(), null, hash);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      setTagRefreshVersion((value) => value + 1);
      setHistoryRefreshVersion((value) => value + 1);
      setHistoryActionNotice(`Created tag ${name.trim()}.`);
    } catch (cause) {
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); throw cause; }
      setHistoryActionError(messageForError(cause));
      if (errorCode(cause) === "tag_refresh_failed") inconsistentManagement("Tag refs changed, but updated state could not be loaded.");
      throw cause;
    } finally { if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); } }
  }

  async function branchAtHead(hash: string): Promise<void> {
    if (repositoryRef.current?.head?.hash !== hash || busyRef.current || managementBusyRef.current > 0 || remoteBusy || operation.kind !== "none") throw new Error("The selected commit is no longer the current HEAD.");
    const owner = repositoryRequest.current, ownerPath = repository!.path;
    const name = window.prompt(`Create local branch at ${hash.slice(0, 10)}:`, "");
    if (!name?.trim()) return;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy(`Creating ${name.trim()}`);
    try {
      const next = await createBranch(activeRepositoryId!, name.trim());
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      dispatchSession({ type: "branchesReady", branches: next });
      setHistoryRefreshVersion((value) => value + 1);
      setHistoryActionNotice(`Created branch ${name.trim()}.`);
    } catch (cause) { if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); throw cause; } setHistoryActionError(messageForError(cause)); if (errorCode(cause) === "branch_refresh_failed") inconsistentManagement("Branch refs changed, but updated state could not be loaded."); throw cause; }
    finally { if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); } }
  }

  async function rebaseOntoSelectedBranch(fullRef: string): Promise<void> {
    if (busyRef.current || managementBusyRef.current > 0 || remoteBusy || !repository || operation.kind !== "none" || conflicts.length > 0) return;
    const owner = repositoryRequest.current, ownerPath = repository.path;
    invalidateWarmValidation();
    busyRef.current = true;
    setBusy("Rebasing current branch");
    setHistoryActionError("");
    try {
      const result = await rebaseOnto(activeRepositoryId!, fullRef);
      if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); return; }
      applyHistoryResult(result);
      if (result.error) throw result.error;
      setHistoryActionNotice(result.state.operation.kind === "none" ? "Rebase complete." : "Rebase stopped. Resolve and stage conflicts in Merge / Conflicts.");
    } catch (cause) { if (owner !== repositoryRequest.current) { cache.current.markStale(ownerPath); throw cause; } setHistoryActionError(messageForError(cause)); if (errorCode(cause) === "merge_refresh_failed") inconsistentManagement("Git ran, but updated state could not be loaded."); throw cause; }
    finally { if (owner === repositoryRequest.current) { busyRef.current = false; setBusy(null); } }
  }

  function applyConflictResult(result: ConflictMutationResult) {
    applyRepositoryState(result.state);
    setSelectedChange(null);
  }

  const changes = status.phase === "ready" ? status.data : null;
  const mergeActive = operation.kind !== "none";
  const repositoryMutationDisabled = mergeActive || conflicts.length > 0 || !sessionReady;
  const hasStageable = !!changes && changes.unstaged.length + changes.untracked.length + changes.conflicted.length > 0;
  const hasStaged = !!changes && changes.staged.length > 0;
  const controlsBusy = !!busy || remoteBusy || managementBusy;
  const changeCount = changes ? changes.staged.length + changes.unstaged.length + changes.untracked.length + changes.conflicted.length : 0;
  const currentTracking = branches.phase === "ready" ? branches.data.branches.find((branch) => branch.kind === "local" && branch.isCurrent) : undefined;
  const activeWorkspace = workspaceCatalog?.workspaces.find((item) => item.id === workspaceCatalog.activeWorkspaceId);

  function cachedHistorySnapshot(path: string, snapshot: HistorySnapshot) {
    if (activeCacheKey.current !== repositoryCacheKey(path) || repository?.path !== path) return;
    const old = cache.current.get(path);
    if (old) cache.current.set(path, { ...old, history: snapshot });
    else cache.current.set(path, {
      session, history: snapshot, activeView, localFilesMode, selectedChange, localDiff,
      commitMessage, conflictVersion, historyVersion, historyRefreshVersion, diffRefreshVersion, tagRefreshVersion, stashRefreshVersion,
      refs: { branches: branches.phase === "ready" ? branches.data : null,
        tags: refBaselines.current.get(repositoryCacheKey(path))?.tags ?? null,
        stashes: refBaselines.current.get(repositoryCacheKey(path))?.stashes ?? null },
      freshness: "fresh", generation: 0,
    });
    refBaselines.current.delete(repositoryCacheKey(path));
    if (snapshot.pageLoaded && repositoryCacheKey(path) === activeCacheKey.current) {
      foregroundHistoryReady.current = true;
      if (foregroundStatusReady.current) { prewarmQueue.current.releaseForeground(); void processPrewarmQueue(); }
    }
  }

  function selectedRepository(workspaceId: string, repositoryId: string) {
    if (activeRepositoryIdRef.current === repositoryId && workspaceCatalog?.activeWorkspaceId === workspaceId) return;
    const item = workspaceCatalog?.workspaces.find((workspace) => workspace.id === workspaceId)?.repositories.find((repo) => repo.id === repositoryId);
    if (!item) return;
    const previous = repositoryRef.current;
    saveActiveSession();
    repositoryRequest.current += 1;
    validationEpoch.current += 1;
    branchRequest.current += 1;
    activeRepositoryIdRef.current = repositoryId;
    setWorkspaceCatalog((current) => current && { ...current, activeWorkspaceId: workspaceId,
      workspaces: current.workspaces.map((workspace) => workspace.id === workspaceId
        ? { ...workspace, activeRepositoryId: repositoryId } : workspace) });
    setMountedViews((current) => current.some((path) => repositoryCacheKey(path) === repositoryCacheKey(item.path))
      ? current : [...current, item.path]);
    cache.current.retainView(item.path);
    setBusy(null); busyRef.current = false;
    managementBusyRef.current = managementCounts.current.get(repositoryId) ?? 0;
    setRevalidating(false); setWorkspaceBusy(false);
    setShowOpen(false); setFileMenu(null);
    lastWatchSequence.current = 0;
    pendingWatch.current = null;
    pendingBackgroundRefresh.current = false;
    prewarmQueue.current.promote(repositoryId);
    if (!restoreCachedSession(item.path)) {
      resetRepositorySession();
      setPath(item.path);
      setShowOpen(false);
      setActiveView("history");
    }
    const request = repositoryRequest.current;
    void getRepositoryInfo(repositoryId).then((info) => {
      if (request === repositoryRequest.current && activeRepositoryIdRef.current === repositoryId) void attachRepository(info, repositoryId);
    }).catch((cause) => {
      if (request === repositoryRequest.current) setOpenError(messageForError(cause));
    });
    // Persist selection in click order. Watcher ownership can follow later;
    // neither persistence nor watcher activation gates the visible view.
    selectionSave.current = selectionSave.current.catch(() => {}).then(() => selectWorkspaceRepository(workspaceId, repositoryId))
      .then(async () => {
        if (activeRepositoryIdRef.current !== repositoryId) return;
        const watched = await activateRepositoryWatch(repositoryId);
        watchSessions.current.set(repositoryId, watched);
        if (activeRepositoryIdRef.current === repositoryId && request === repositoryRequest.current) {
          repositoryRef.current = watched;
          dispatchSession({ type: "opened", repository: watched });
        }
      }).catch((cause) => { if (activeRepositoryIdRef.current === repositoryId) setWorkspaceError(messageForError(cause)); });
    if (previous) cache.current.markStale(previous.path);
  }

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

  return <WorkspaceLayout repository={repository} repositoryError={refreshError || (status.phase === "error" ? status.message : undefined)} activeView={activeView} onViewChange={(view) => { setShowOpen(false); setActiveView(view); }} onOpen={() => setShowOpen(true)} openDisabled={controlsBusy} changeCount={changeCount} tracking={currentTracking ? { ahead: currentTracking.ahead, behind: currentTracking.behind } : undefined} busy={busy ?? (revalidating ? "Refreshing repository…" : null)}
    workspaceName={activeWorkspace?.name ?? "Default"}
    workspaceMenu={<WorkspacePanel catalog={workspaceCatalog} busy={workspaceBusy || controlsBusy} error={workspaceError}
      onCreate={(name) => void runWorkspaceTransition("Creating workspace", () => createWorkspace(name))}
      onRename={(id, name) => void renameCurrentWorkspace(id, name)}
      onDelete={(id) => void runWorkspaceTransition("Deleting workspace", () => deleteWorkspace(id))}
      onSwitch={(id) => { const target = workspaceCatalog?.workspaces.find((item) => item.id === id); const repo = target?.repositories.find((item) => item.id === target.activeRepositoryId); void runWorkspaceTransition("Switching workspace", () => switchWorkspace(id), repo ? { workspaceId: id, repositoryId: repo.id, path: repo.path } : null); }}
      onSelectRepository={selectedRepository}
      onRemoveRepository={(workspaceId, repositoryId) => void runWorkspaceTransition("Removing repository", () => removeRepository(workspaceId, repositoryId))} />}
    repositoryTabs={activeWorkspace?.repositories.map((item) => <button type="button" key={item.id} className={`woo-repo-tab ${activeWorkspace.activeRepositoryId === item.id ? "active" : ""}`} title={item.path} onClick={() => selectedRepository(activeWorkspace.id, item.id)}>{activeWorkspace.activeRepositoryId === item.id && changeCount > 0 && <span className="woo-dirty">●</span>}{item.path.split(/[\\/]/).filter(Boolean).at(-1) ?? item.path}</button>)}
    activity={(open) => <OperationHistory active={open} repositoryNames={Object.fromEntries(workspaceCatalog?.workspaces.flatMap((workspace) => workspace.repositories.map((item) => [item.id, item.path.split(/[\\/]/).filter(Boolean).at(-1) ?? item.path])) ?? [])} />}
    remoteControls={mountedViews.map((viewPath) => { const active = !!repository && repositoryCacheKey(repository.path) === repositoryCacheKey(viewPath); return <RepositoryView key={repositoryCacheKey(viewPath)} repositoryId={repositoryIdForPath(viewPath)} active={active} ready={active && sessionReady} className="woo-remote-view">
      {active && sessionReady ? <RemotePanel initialRemotes={cache.current.get(viewPath)?.refs.remotes?.remotes} canPullPush={branches.phase === "ready" && branches.data.branches.some((branch) => branch.kind === "local" && branch.isCurrent && !!branch.upstream)} onComplete={(result) => applyRemoteRefreshFor(repositoryIdForPath(viewPath), viewPath, result)} onInconsistent={(message) => { if (activeRepositoryIdRef.current === repositoryIdForPath(viewPath)) inconsistentManagement(message); else cache.current.markStale(viewPath); }} onBusyChange={(value) => remoteBusyChanged(repositoryIdForPath(viewPath), value)} localBusy={!!busy || managementBusy || repositoryMutationDisabled} /> : null}
    </RepositoryView>; })}
    sidebar={mountedViews.map((viewPath) => { const active = !!repository && repositoryCacheKey(repository.path) === repositoryCacheKey(viewPath); return <RepositoryView key={repositoryCacheKey(viewPath)} repositoryId={repositoryIdForPath(viewPath)} active={active} ready={active && sessionReady} className="woo-sidebar-view">
      {active && repository ? <BranchPanel compact state={branches} currentBranch={repository.branch} busy={controlsBusy || repositoryMutationDisabled} onCreate={createLocalBranch} onCheckout={(name) => void switchBranch(name)} onRename={(ref, name) => changeBranchRef(`Renaming ${ref}`, () => renameBranch(activeRepositoryId!, ref, name))} onDelete={(ref) => changeBranchRef(`Deleting ${ref}`, () => deleteBranch(activeRepositoryId!, ref))} onRetry={() => void loadBranches()} error={branchError} /> : null}
    </RepositoryView>; })}>
    {!repository && activeRepositoryId && !showOpen && <div className="woo-open-view"><p className="status-placeholder">Loading repository…</p>{openError && <p className="error" role="alert">{openError}</p>}</div>}
    {(showOpen || (!repository && !activeRepositoryId)) && <div className="woo-open-view">
        <div className="intro"><p className="eyebrow">YOUR WORKSPACE</p><h1>Workspaces and repositories</h1><p>Choose a repository to work on.</p></div>
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
      {mountedViews.map((viewPath) => { const active = !!repository && repositoryCacheKey(repository.path) === repositoryCacheKey(viewPath); return <RepositoryView key={repositoryCacheKey(viewPath)} repositoryId={repositoryIdForPath(viewPath)} active={active} ready={active && sessionReady} className="woo-repository-view">
      {active && repository ? <>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "branches"}>
        <section className="repo-card" aria-label="Repository information">
          <div className="repo-heading"><span className="repo-icon">⌘</span><div><p className="eyebrow">REPOSITORY</p><h2>{repository.path.split(/[\\/]/).filter(Boolean).at(-1)}</h2></div></div>
          <dl>
            <div><dt>Path</dt><dd className="path-value">{repository.path}</dd></div>
            <div><dt>Current branch</dt><dd>{repository.branch ?? "Detached HEAD"}</dd></div>
            <div><dt>HEAD</dt><dd>{repository.head ? <><code>{repository.head.hash.slice(0, 10)}</code><span className="subject">{repository.head.subject}</span></> : "No commits yet"}</dd></div>
          </dl>
        </section>
        <BranchPanel state={branches} currentBranch={repository.branch} busy={controlsBusy || repositoryMutationDisabled} onCreate={createLocalBranch} onCheckout={(name) => void switchBranch(name)} onRename={(ref, name) => changeBranchRef(`Renaming ${ref}`, () => renameBranch(activeRepositoryId!, ref, name))} onDelete={(ref) => changeBranchRef(`Deleting ${ref}`, () => deleteBranch(activeRepositoryId!, ref))} onRetry={() => void loadBranches()} error={branchError} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "merge"}>
        <MergePanel branches={branches.phase === "ready" ? branches.data.branches : []} operation={operation} conflicts={conflicts} busy={controlsBusy || status.phase !== "ready" || repositoryMutationDisabled} refreshToken={conflictVersion} onBusyChange={(value) => managementBusyChanged(repositoryIdForPath(viewPath), value)} onMerge={applyMergeResult} onHistory={applyHistoryResult} onConflict={applyConflictResult} onInconsistent={inconsistentManagement} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "stashes"}>
        <StashPanel refreshToken={stashRefreshVersion} busy={controlsBusy || repositoryMutationDisabled} onBusyChange={(value) => managementBusyChanged(repositoryIdForPath(viewPath), value)} onMutation={applyStashMutation} onInconsistent={inconsistentManagement} />
        </div>
        <div className="woo-view woo-manage-view" hidden={showOpen || activeView !== "tags"}>
        <TagPanel refreshToken={tagRefreshVersion} busy={controlsBusy || repositoryMutationDisabled} onBusyChange={(value) => managementBusyChanged(repositoryIdForPath(viewPath), value)} onMutation={() => setHistoryRefreshVersion((value) => value + 1)} onInconsistent={inconsistentManagement} />
        </div>
        <section className="changes-panel woo-view" aria-label="Working tree changes" hidden={showOpen || activeView !== "changes"}>
          <div className="woo-changes-list">
          <div className="changes-heading"><div><p className="eyebrow">WORKING TREE</p><h2>Changes</h2></div><FileViewToggle mode={localFilesMode} onChange={setLocalFilesMode} /><div className="change-actions">
            <button className="secondary" disabled={controlsBusy} onClick={() => void refresh()}>Refresh</button>
            <button className="secondary" disabled={controlsBusy || repositoryMutationDisabled || !hasStaged} onClick={() => void changeIndex("Unstaging all", () => unstageAll(activeRepositoryId!))}>Unstage All</button>
            <button disabled={controlsBusy || repositoryMutationDisabled || !hasStageable} onClick={() => void changeIndex("Staging all", () => stageAll(activeRepositoryId!))}>Stage All</button>
          </div></div>
          {busy && <p className="operation-feedback" role="status">{busy}…</p>}
          {status.phase === "loading" && <p className="status-placeholder">Loading changes…</p>}
          {status.phase === "error" && <p className="error" role="alert">{status.message}</p>}
          {changes && <div className="change-groups">
            <FileGroup title="Staged" files={changes.staged} mode={localFilesMode} selectedPath={selectedChange?.source === "staged" ? selectedChange.change.path : null} action="Unstage" onSelect={(file) => { setSelectedChange({ source: "staged", change: file }); setLocalDiff(null); }} onContextMenu={(file, event) => showFileMenu("staged", file, event)} onAction={(file) => void changeIndex(`Unstaging ${file.path}`, () => unstageFile(activeRepositoryId!, file))} busy={controlsBusy || repositoryMutationDisabled} />
            <FileGroup title="Unstaged" files={changes.unstaged} mode={localFilesMode} selectedPath={selectedChange?.source === "unstaged" ? selectedChange.change.path : null} action="Stage" onSelect={(file) => { setSelectedChange({ source: "unstaged", change: file }); setLocalDiff(null); }} onContextMenu={(file, event) => showFileMenu("unstaged", file, event)} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(activeRepositoryId!, file))} busy={controlsBusy || repositoryMutationDisabled} />
            <FileGroup title="Untracked" files={changes.untracked} mode={localFilesMode} selectedPath={selectedChange?.source === "untracked" ? selectedChange.change.path : null} action="Stage" onSelect={(file) => { setSelectedChange({ source: "untracked", change: file }); setLocalDiff(null); }} onContextMenu={(file, event) => showFileMenu("untracked", file, event)} onAction={(file) => void changeIndex(`Staging ${file.path}`, () => stageFile(activeRepositoryId!, file))} busy={controlsBusy || repositoryMutationDisabled} />
            {changes.conflicted.length > 0 && <FileGroup title="Conflicted" files={changes.conflicted} mode={localFilesMode} busy={controlsBusy || repositoryMutationDisabled} />}
          </div>}
          <div className="commit-area">
            <div><h3>Commit staged changes</h3><p>Only files in Staged are included in this commit.</p></div>
            <label htmlFor="commit-message">Commit message</label>
            <textarea id="commit-message" value={commitMessage} disabled={controlsBusy || repositoryMutationDisabled} rows={4} placeholder="Describe your change" onChange={(event) => { setCommitMessage(event.target.value); setCommitError(""); setCommitNotice(""); }} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void commit(); } }} />
            <div className="commit-footer"><span>{revalidating || !sessionReady ? "Refreshing repository state…" : repositoryMutationDisabled ? "Finish the current repository operation above." : !hasStaged ? "Stage a change to enable commit." : "Ctrl+Enter to commit"}</span><button disabled={controlsBusy || repositoryMutationDisabled || !hasStaged || status.phase !== "ready"} onClick={() => void commit()}>{busy === "Committing staged changes" ? "Committing…" : "Commit"}</button></div>
            {commitError && <p className="error" role="alert">{commitError}</p>}
            {commitNotice && <p className="commit-notice" role="status">{commitNotice}</p>}
          </div>
          </div>
          <div className="woo-changes-detail">
            {selectedChange && changes ? <DiffViewer key={`${selectedChange.source}:${selectedChange.change.path}:${selectedChange.change.oldPath ?? ""}:${diffRefreshVersion}`} source={selectedChange.source} change={selectedChange.change} enabled={sessionReady} initialDiff={localDiff} onDiff={setLocalDiff} busy={controlsBusy || repositoryMutationDisabled} onPartial={selectedChange.source === "staged" || selectedChange.source === "unstaged" ? (selection) => changePart(selectedChange.source as "staged" | "unstaged", selectedChange.change, selection) : undefined} /> : <p className="woo-detail-empty">Select a changed file to view its diff.</p>}
          </div>
        </section>
        <div className="woo-view woo-history-view" hidden={showOpen || activeView !== "history"}>
          {historyActionError && <p className="error" role="alert">{historyActionError}</p>}{historyActionNotice && <p className="commit-notice" role="status">{historyActionNotice}</p>}
          <HistoryPanel key={repositoryCacheKey(repository.path)} initialSnapshot={cache.current.get(repository.path)?.history} onSnapshot={(snapshot) => cachedHistorySnapshot(repository.path, snapshot)} enabled={sessionReady} resetToken={historyVersion} refreshToken={historyRefreshVersion} actionBusy={controlsBusy || repositoryMutationDisabled} onAction={runSelectedCommitAction} onTag={tagSelectedCommit} onBranch={branchAtHead} currentHead={repository.head?.hash} rebaseTargets={branches.phase === "ready" ? branches.data.branches.filter((branch) => branch.kind === "local" && !branch.isCurrent).map((branch) => ({ name: branch.fullRefName, hash: branch.targetHash })) : []} onRebase={rebaseOntoSelectedBranch} />
        </div>
      </> : null}
      </RepositoryView>; })}
      {fileMenu && <ContextMenu x={fileMenu.x} y={fileMenu.y} title={fileMenu.change.path} onClose={() => setFileMenu(null)} actions={[
        { label: fileMenu.source === "staged" ? "Unstage" : "Stage", disabled: controlsBusy || repositoryMutationDisabled, onSelect: () => { const { source, change } = fileMenu; void changeIndex(`${source === "staged" ? "Unstaging" : "Staging"} ${change.path}`, () => source === "staged" ? unstageFile(activeRepositoryId!, change) : stageFile(activeRepositoryId!, change)); } },
        { label: fileMenu.source === "staged" ? "Unstage selected lines…" : "Stage selected lines…", disabled: fileMenu.source === "untracked" || controlsBusy || repositoryMutationDisabled, title: "Select changed lines in the diff pane", onSelect: () => setSelectedChange({ source: fileMenu.source, change: fileMenu.change }) },
        { label: "Discard changes…", disabled: true, title: "Woo does not currently expose a safe discard operation", danger: true, onSelect: () => {} },
        { label: "Open file", disabled: true, title: "External file opening is not yet available", onSelect: () => {} },
        { label: "Open containing folder", disabled: true, title: "External folder opening is not yet available", onSelect: () => {} },
        { label: "Copy path", onSelect: () => void (navigator.clipboard?.writeText(fullFilePath(fileMenu.change)) ?? Promise.reject()).catch(() => setCommitError("Could not copy the file path.")) },
      ]} />}
  </WorkspaceLayout>;
}
