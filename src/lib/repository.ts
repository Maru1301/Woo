import { invoke } from "@tauri-apps/api/core";

export interface HeadInfo { hash: string; subject: string; authorDate: string }
export interface RepositoryInfo { path: string; branch: string | null; head: HeadInfo | null; openDurationMs: number }
export interface AppError { code: string; message: string }
export type ChangeKind = "added" | "modified" | "deleted" | "renamed" | "copied" | "type_changed" | "conflicted" | "untracked";
export interface FileChange { path: string; oldPath: string | null; kind: ChangeKind }
export interface RepositoryStatus { staged: FileChange[]; unstaged: FileChange[]; untracked: FileChange[]; conflicted: FileChange[] }
export interface CommitResult { head: HeadInfo; status: RepositoryStatus }
export type BranchKind = "local" | "remote";
export interface BranchInfo { name: string; fullRefName: string; kind: BranchKind; isCurrent: boolean; targetHash: string; upstream: string | null }
export interface BranchList { branches: BranchInfo[] }
export interface CheckoutResult { branch: string | null; head: HeadInfo; status: RepositoryStatus; branches: BranchList }
export interface RemoteInfo { name: string; fetchUrl: string | null; pushUrl: string | null }
export interface RemoteList { remotes: RemoteInfo[] }
export interface RemoteRefresh { branches: BranchList; head: HeadInfo | null; status: RepositoryStatus | null; resetHistory: boolean; refreshHistory: boolean; clearDiff: boolean }
export type RemoteKind = "fetch" | "pull" | "push";
export type RemotePhase = "queued" | "running" | "completed" | "failed" | "cancelled";
export interface RemoteOperationStatus { id: number; kind: RemoteKind; phase: RemotePhase; startedAtMs: number; elapsedMs: number; gitDurationMs: number | null; refreshDurationMs: number | null; refresh: RemoteRefresh | null; error: AppError | null }
export interface CommitInfo { hash: string; parentHashes: string[]; authorName: string; authorEmail: string; timestamp: string; subject: string; refs: string[] }
export interface GraphRow { nodeLane: number; laneCount: number; incoming: boolean; continuations: number[]; parentLanes: number[] }
export interface CommitHistoryPage { commits: CommitInfo[]; graphRows: GraphRow[]; nextCursor: string | null; hasMore: boolean }
export type DiffLineKind = "context" | "addition" | "deletion" | "no_newline";
export interface DiffLine { kind: DiffLineKind; oldLineNumber: number | null; newLineNumber: number | null; content: string }
export interface DiffHunk { oldStart: number; oldCount: number; newStart: number; newCount: number; header: string; lines: DiffLine[] }
export interface DiffFile { change: FileChange; isBinary: boolean; hunks: DiffHunk[] }

export function getUnstagedDiff(change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_unstaged_diff", { change }); }
export function getStagedDiff(change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_staged_diff", { change }); }
export function getUntrackedDiff(change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_untracked_diff", { change }); }
export function getCommitFiles(commit: string): Promise<FileChange[]> { return invoke<FileChange[]>("get_commit_files", { commit }); }
export function getCommitDiff(commit: string, change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_commit_diff", { commit, change }); }

export function getCommitHistory(cursor: string | null = null): Promise<CommitHistoryPage> {
  return invoke<CommitHistoryPage>("get_commit_history", { cursor });
}

export function openRepository(path: string): Promise<RepositoryInfo> {
  return invoke<RepositoryInfo>("open_repository", { path });
}

export function getBranches(): Promise<BranchList> { return invoke<BranchList>("get_branches"); }
export function createBranch(name: string): Promise<BranchList> { return invoke<BranchList>("create_branch", { name }); }
export function checkoutBranch(name: string): Promise<CheckoutResult> { return invoke<CheckoutResult>("checkout_branch", { name }); }
export function getRemotes(): Promise<RemoteList> { return invoke<RemoteList>("get_remotes"); }
export function startFetch(remote: string): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_fetch", { remote }); }
export function startPull(): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_pull"); }
export function startPush(): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_push"); }
export function getRemoteOperation(id: number): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("get_remote_operation", { id }); }
export function cancelRemoteOperation(id: number): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("cancel_remote_operation", { id }); }

export function getRepositoryStatus(): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("get_repository_status");
}

export function stageFile(change: FileChange): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("stage_file", { path: change.path, oldPath: change.kind === "renamed" ? change.oldPath : null });
}

export function unstageFile(change: FileChange): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("unstage_file", { path: change.path, oldPath: change.kind === "renamed" ? change.oldPath : null });
}

export function stageAll(): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("stage_all");
}

export function unstageAll(): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("unstage_all");
}

export function commitStaged(message: string): Promise<CommitResult> {
  return invoke<CommitResult>("commit_staged", { message });
}

export function messageForError(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as AppError).message;
    if (typeof message === "string") return message;
  }
  return "The operation failed.";
}

export function errorCode(error: unknown): string | null {
  if (typeof error === "object" && error !== null && "code" in error && typeof (error as AppError).code === "string") {
    return (error as AppError).code;
  }
  return null;
}
