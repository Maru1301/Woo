import type { WorkspaceCatalog } from "../../features/workspace/workspace";
import type { BranchList, DiffFile, FileChange, RemoteList, RepositoryInfo, RepositoryState, StashList, TagList } from "../../lib/repository";
import type { DiffSource } from "../../features/diff/DiffViewer";
import type { FileViewMode } from "../../features/changes/FilePresentation";
import type { HistorySnapshot } from "../../features/history/HistoryPanel";
import type { WorkspaceView } from "../../layout/WorkspaceLayout";
import type { RepositorySessionState } from "./useRepositorySession";

// Workspace IDs identify memberships. Backend workspace paths are canonical and
// identify the Git data shared by the same repository in multiple workspaces.
export function repositoryCacheKey(path: string): string {
  return /^[a-zA-Z]:[\\/]/.test(path) || path.startsWith("\\\\")
    ? path.replaceAll("/", "\\").toLowerCase()
    : path;
}

export interface CachedRepository {
  session: RepositorySessionState;
  history: HistorySnapshot | null;
  activeView: WorkspaceView;
  localFilesMode: FileViewMode;
  selectedChange: { source: DiffSource; change: FileChange } | null;
  localDiff: DiffFile | null;
  commitMessage: string;
  conflictVersion: number;
  historyVersion: number;
  historyRefreshVersion: number;
  diffRefreshVersion: number;
  tagRefreshVersion: number;
  stashRefreshVersion: number;
  refs: { branches: BranchList | null; tags: TagList | null; stashes: StashList | null; remotes?: RemoteList | null };
  freshness: "fresh" | "stale" | "refreshing";
  generation: number;
}

export class RepositoryCache {
  private entries = new Map<string, CachedRepository>();
  private warmStates = new Map<string, "cold" | "warming" | "ready" | "stale">();
  private retainedViews = new Set<string>();
  constructor(private readonly limit = 6) {}

  retainView(path: string): void { this.retainedViews.add(repositoryCacheKey(path)); }
  releaseView(path: string): void { this.retainedViews.delete(repositoryCacheKey(path)); this.trim(); }
  private trim(): void {
    while (this.entries.size > this.limit) {
      const candidate = [...this.entries.keys()].find((key) => !this.retainedViews.has(key));
      if (!candidate) break; // All remaining entries back mounted views.
      this.entries.delete(candidate);
      this.warmStates.delete(candidate);
    }
  }

  get(path: string): CachedRepository | undefined {
    const key = repositoryCacheKey(path);
    const value = this.entries.get(key);
    if (value) { this.entries.delete(key); this.entries.set(key, value); }
    return value;
  }

  set(path: string, value: CachedRepository): void {
    const key = repositoryCacheKey(path);
    this.entries.delete(key);
    this.entries.set(key, value);
    this.warmStates.set(key, value.freshness === "stale" ? "stale" : "ready");
    this.trim();
  }

  warmState(path: string): "cold" | "warming" | "ready" | "stale" {
    return this.warmStates.get(repositoryCacheKey(path)) ?? "cold";
  }

  beginWarm(path: string): boolean {
    if (this.warmState(path) !== "cold") return false;
    this.warmStates.set(repositoryCacheKey(path), "warming");
    return true;
  }

  failWarm(path: string): void {
    if (this.warmState(path) === "warming") this.warmStates.set(repositoryCacheKey(path), "cold");
  }

  update(path: string, update: (value: CachedRepository) => CachedRepository): void {
    const current = this.get(path);
    if (current) this.set(path, update(current));
  }

  markStale(path: string): void {
    this.update(path, (value) => ({ ...value, freshness: "stale", generation: value.generation + 1 }));
    if (this.entries.has(repositoryCacheKey(path))) this.warmStates.set(repositoryCacheKey(path), "stale");
  }

  beginRefresh(path: string): number | null {
    const current = this.get(path);
    if (!current) return null;
    const generation = current.generation + 1;
    this.set(path, { ...current, generation, freshness: "refreshing" });
    return generation;
  }

  finishRefresh(path: string, generation: number, update: (value: CachedRepository) => CachedRepository): boolean {
    const current = this.get(path);
    if (!current || current.generation !== generation) return false;
    this.set(path, update(current));
    return true;
  }

  prune(catalog: WorkspaceCatalog): void {
    const paths = new Set(catalog.workspaces.flatMap((workspace) => workspace.repositories.map((repo) => repositoryCacheKey(repo.path))));
    for (const key of this.entries.keys()) if (!paths.has(key)) this.entries.delete(key);
    for (const key of this.warmStates.keys()) if (!paths.has(key)) this.warmStates.delete(key);
    for (const key of this.retainedViews) if (!paths.has(key)) this.retainedViews.delete(key);
  }
}

/// A small startup queue. Foreground initialization is owned by App; the queue
/// only admits one background repository at a time after App releases it.
export class RepositoryPrewarmQueue {
  private waiting: { id: string; path: string }[] = [];
  private foregroundReady = false;

  restore(repositories: { id: string; path: string }[], activeId: string | null): void {
    this.waiting = repositories.filter((item) => item.id !== activeId);
    this.foregroundReady = false;
  }

  releaseForeground(): void { this.foregroundReady = true; }

  promote(id: string): { id: string; path: string } | null {
    const index = this.waiting.findIndex((item) => item.id === id);
    return index < 0 ? null : this.waiting.splice(index, 1)[0];
  }

  nextBackground(): { id: string; path: string } | null {
    return this.foregroundReady ? this.waiting.shift() ?? null : null;
  }
}

export function sameIdentity(a: RepositoryInfo | null, b: RepositoryInfo): boolean {
  return a?.head?.hash === b.head?.hash && a?.branch === b.branch;
}

export function needsColdHistoryLoad(snapshot: HistorySnapshot | null | undefined): boolean {
  return !snapshot?.pageLoaded;
}

export function retainedCommitHash(current: string | null, pageHashes: readonly string[], validatedHash: string | null, validatedExists: boolean): string | null {
  return current && (pageHashes.includes(current) || current === validatedHash && validatedExists) ? current : null;
}

export function refsChanged(cached: CachedRepository["refs"], next: { branches: BranchList; tags: TagList; stashes: StashList }): boolean {
  return !cached.branches || !cached.tags || !cached.stashes
    || JSON.stringify(cached.branches) !== JSON.stringify(next.branches)
    || JSON.stringify(cached.tags) !== JSON.stringify(next.tags)
    || JSON.stringify(cached.stashes) !== JSON.stringify(next.stashes);
}

export function sameRepositoryState(a: RepositoryState, b: RepositoryState): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
