import { invoke } from "@tauri-apps/api/core";

export interface HeadInfo { hash: string; subject: string; authorDate: string }
export interface RepositoryInfo { path: string; branch: string | null; head: HeadInfo | null; openDurationMs: number; sessionId: number; watchWarning: string | null }
export interface AppError { code: string; message: string }
export type ChangeKind = "added" | "modified" | "deleted" | "renamed" | "copied" | "type_changed" | "conflicted" | "untracked";
export interface FileChange { path: string; oldPath: string | null; kind: ChangeKind }
export interface RepositoryStatus { staged: FileChange[]; unstaged: FileChange[]; untracked: FileChange[]; conflicted: FileChange[] }
export type RepositoryOperation = { kind: "none" } | { kind: "merge"; mergeHeads: string[]; message: string }
  | { kind: "rebase"; currentCommit: string | null; step: number | null; total: number | null }
  | { kind: "cherry_pick"; commit: string } | { kind: "revert"; commit: string };
export type ConflictKind = "both_modified" | "both_added" | "deleted_by_us" | "deleted_by_them" | "added_by_us" | "added_by_them" | "other";
export interface ConflictStage { objectHash: string; mode: string }
export interface ConflictFile { path: string; kind: ConflictKind; base: ConflictStage | null; ours: ConflictStage | null; theirs: ConflictStage | null }
export interface RepositoryState { status: RepositoryStatus; operation: RepositoryOperation; conflicts: ConflictFile[] }
export interface AutoRefreshEvent { repositoryId: string | null; sessionId: number; sequence: number; state: RepositoryState | null; branch: string | null | undefined; head: HeadInfo | null | undefined; branches: BranchList | null; resetHistory: boolean; refreshHistory: boolean; refreshTags: boolean; clearDiff: boolean; diffPaths: string[]; unavailable: AppError | null; coalescedEvents: number; validationMs: number; gitProcessCount: number }
export interface ContentPart { text: string | null; isBinary: boolean; oversized: boolean }
export interface ConflictContent { path: string; base: ContentPart | null; ours: ContentPart | null; theirs: ContentPart | null; working: ContentPart | null }
export type MergeOutcome = "already_up_to_date" | "fast_forward" | "clean_merge" | "needs_resolution" | "needs_completion" | "failed" | "completed" | "aborted";
export interface MergeMutationResult { state: RepositoryState; branches: BranchList; branch: string | null; head: HeadInfo; outcome: MergeOutcome; error: AppError | null; resetHistory: boolean; clearDiff: boolean; gitDurationMs: number | null; refreshDurationMs: number }
export interface HistoryMutationResult { state: RepositoryState; branches: BranchList; branch: string | null; head: HeadInfo; error: AppError | null; resetHistory: boolean; clearDiff: boolean; gitDurationMs: number | null; refreshDurationMs: number }
export type ResetMode = "soft" | "mixed" | "hard";
export interface ConflictMutationResult { state: RepositoryState; error: AppError | null }
export interface CommitResult { head: HeadInfo; status: RepositoryStatus }
export type BranchKind = "local" | "remote";
export interface BranchInfo { name: string; fullRefName: string; kind: BranchKind; isCurrent: boolean; targetHash: string; upstream: string | null; ahead: number | null; behind: number | null }
export interface BranchList { branches: BranchInfo[] }
export interface CheckoutResult { branch: string | null; head: HeadInfo; status: RepositoryStatus; branches: BranchList }
export interface BranchRefMutationResult { branch: string | null; branches: BranchList }
export interface StashInfo { reference: string; commitHash: string; message: string; timestamp: string }
export interface StashList { stashes: StashInfo[] }
export interface StashMutationResult { stashes: StashList; status: RepositoryStatus | null; operation: RepositoryOperation | null; conflicts: ConflictFile[] | null; resetHistory: boolean; clearDiff: boolean; error: AppError | null }
export interface TagInfo { name: string; targetHash: string; kind: "lightweight" | "annotated" }
export interface TagList { tags: TagInfo[] }
export interface TagMutationResult { tags: TagList }
export interface RemoteInfo { name: string; fetchUrl: string | null; pushUrl: string | null }
export interface RemoteList { remotes: RemoteInfo[] }
export interface RemoteRefresh { branches: BranchList; head: HeadInfo | null; status: RepositoryStatus | null; operation: RepositoryOperation | null; conflicts: ConflictFile[] | null; resetHistory: boolean; refreshHistory: boolean; clearDiff: boolean }
export type RemoteKind = "fetch" | "pull" | "push";
export type RemotePhase = "queued" | "running" | "completed" | "failed" | "cancelled" | "timed_out";
export interface RemoteOperationStatus { id: number; sessionId: number; source: "user" | "background"; kind: RemoteKind; phase: RemotePhase; startedAtMs: number; elapsedMs: number; gitDurationMs: number | null; refreshDurationMs: number | null; refresh: RemoteRefresh | null; error: AppError | null }
export interface OperationEntry { id: number; repositoryId: string; kind: string; source: "user" | "background"; startedAtMs: number; finishedAtMs: number | null; durationMs: number | null; phase: "running" | "completed" | "failed" | "cancelled" | "timed_out"; summary: string; diagnostics: string | null }
export function getOperationHistory(): Promise<OperationEntry[]> { return invoke<OperationEntry[]>("get_operation_history"); }
export interface CommitInfo { hash: string; parentHashes: string[]; authorName: string; authorEmail: string; timestamp: string; subject: string; refs: string[] }
export interface GraphRow { nodeLane: number; laneCount: number; incoming: boolean; continuations: number[]; parentLanes: number[] }
export interface CommitHistoryPage { commits: CommitInfo[]; graphRows: GraphRow[]; nextCursor: string | null; hasMore: boolean }
export type DiffLineKind = "context" | "addition" | "deletion" | "no_newline";
export interface DiffLine { kind: DiffLineKind; oldLineNumber: number | null; newLineNumber: number | null; content: string }
export interface DiffHunk { oldStart: number; oldCount: number; newStart: number; newCount: number; header: string; lines: DiffLine[] }
export interface DiffFile { change: FileChange; isBinary: boolean; hunks: DiffHunk[]; revision: string; partialStageable: boolean }
export interface PartialSelection { revision: string; hunkIndex: number; lineIndices: number[] | null }
export interface PartialStageResult { status: RepositoryStatus; stagedDiff: DiffFile | null; unstagedDiff: DiffFile | null; error: AppError | null }

export function getRepositoryState(repositoryId: string): Promise<RepositoryState> { return invoke<RepositoryState>("get_repository_state", { repositoryId }); }
export function revalidateRepository(repositoryId: string): Promise<void> { return invoke<void>("revalidate_repository", { repositoryId }); }
export function getConflictContent(repositoryId: string, path: string): Promise<ConflictContent> { return invoke<ConflictContent>("get_conflict_content", { repositoryId, path }); }
export function mergeBranch(repositoryId: string, fullRef: string): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("merge_branch", { repositoryId, fullRef }); }
export function completeMerge(repositoryId: string, message: string): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("complete_merge", { repositoryId, message }); }
export function abortMerge(repositoryId: string): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("abort_merge", { repositoryId }); }
export function rebaseOnto(repositoryId: string, fullRef: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("rebase_onto", { repositoryId, fullRef }); }
export function cherryPick(repositoryId: string, commit: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("cherry_pick", { repositoryId, commit }); }
export function revertCommit(repositoryId: string, commit: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("revert_commit", { repositoryId, commit }); }
export function resetTo(repositoryId: string, commit: string, mode: ResetMode): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("reset_to", { repositoryId, commit, mode }); }
export function continueHistoryOperation(repositoryId: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("continue_history_operation", { repositoryId }); }
export function skipHistoryOperation(repositoryId: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("skip_history_operation", { repositoryId }); }
export function abortHistoryOperation(repositoryId: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("abort_history_operation", { repositoryId }); }
export function saveConflictText(repositoryId: string, path: string, expected: string | null, text: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("save_conflict_text", { repositoryId, path, expected, text }); }
export function useConflictSide(repositoryId: string, path: string, side: "ours" | "theirs"): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("use_conflict_side", { repositoryId, path, side }); }
export function stageConflict(repositoryId: string, path: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("stage_conflict", { repositoryId, path }); }
export function deleteConflict(repositoryId: string, path: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("delete_conflict", { repositoryId, path }); }

export function getUnstagedDiff(repositoryId: string, change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_unstaged_diff", { repositoryId, change }); }
export function partialStage(repositoryId: string, path: string, staged: boolean, selection: PartialSelection): Promise<PartialStageResult> { return invoke<PartialStageResult>("partial_stage", { repositoryId, path, staged, selection }); }
export function getStagedDiff(repositoryId: string, change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_staged_diff", { repositoryId, change }); }
export function getUntrackedDiff(repositoryId: string, change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_untracked_diff", { repositoryId, change }); }
export function getCommitFiles(repositoryId: string, commit: string): Promise<FileChange[]> { return invoke<FileChange[]>("get_commit_files", { repositoryId, commit }); }
export function getCommitDiff(repositoryId: string, commit: string, change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_commit_diff", { repositoryId, commit, change }); }

export function getCommitHistory(repositoryId: string, cursor: string | null = null): Promise<CommitHistoryPage> {
  return invoke<CommitHistoryPage>("get_commit_history", { repositoryId, cursor });
}

export function getRepositoryInfo(repositoryId: string): Promise<RepositoryInfo> {
  return invoke<RepositoryInfo>("get_repository_info", { repositoryId });
}

export function getBranches(repositoryId: string): Promise<BranchList> { return invoke<BranchList>("get_branches", { repositoryId }); }
export function createBranch(repositoryId: string, name: string): Promise<BranchList> { return invoke<BranchList>("create_branch", { repositoryId, name }); }
export function checkoutBranch(repositoryId: string, name: string): Promise<CheckoutResult> { return invoke<CheckoutResult>("checkout_branch", { repositoryId, name }); }
export function renameBranch(repositoryId: string, fullRef: string, newName: string): Promise<BranchRefMutationResult> { return invoke<BranchRefMutationResult>("rename_branch", { repositoryId, fullRef, newName }); }
export function deleteBranch(repositoryId: string, fullRef: string): Promise<BranchRefMutationResult> { return invoke<BranchRefMutationResult>("delete_branch", { repositoryId, fullRef }); }
export function getStashes(repositoryId: string): Promise<StashList> { return invoke<StashList>("get_stashes", { repositoryId }); }
export function createStash(repositoryId: string, message: string | null): Promise<StashMutationResult> { return invoke<StashMutationResult>("create_stash", { repositoryId, message }); }
export function applyStash(repositoryId: string, hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("apply_stash", { repositoryId, hash }); }
export function popStash(repositoryId: string, hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("pop_stash", { repositoryId, hash }); }
export function dropStash(repositoryId: string, hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("drop_stash", { repositoryId, hash }); }
export function getTags(repositoryId: string): Promise<TagList> { return invoke<TagList>("get_tags", { repositoryId }); }
export function createTag(repositoryId: string, name: string, annotation: string | null, targetHash: string | null): Promise<TagMutationResult> { return invoke<TagMutationResult>("create_tag", { repositoryId, name, annotation, targetHash }); }
export function deleteTag(repositoryId: string, name: string): Promise<TagMutationResult> { return invoke<TagMutationResult>("delete_tag", { repositoryId, name }); }
export function getRemotes(repositoryId: string): Promise<RemoteList> { return invoke<RemoteList>("get_remotes", { repositoryId }); }
export function startFetch(repositoryId: string, remote: string): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_fetch", { repositoryId, remote }); }
export function startPull(repositoryId: string): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_pull", { repositoryId }); }
export function startPush(repositoryId: string): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("start_push", { repositoryId }); }
export function getRemoteOperation(repositoryId: string, id: number): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("get_remote_operation", { repositoryId, id }); }
export function cancelRemoteOperation(repositoryId: string, id: number): Promise<RemoteOperationStatus> { return invoke<RemoteOperationStatus>("cancel_remote_operation", { repositoryId, id }); }

export function getRepositoryStatus(repositoryId: string): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("get_repository_status", { repositoryId });
}

export function stageFile(repositoryId: string, change: FileChange): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("stage_file", { repositoryId, path: change.path, oldPath: change.kind === "renamed" ? change.oldPath : null });
}

export function unstageFile(repositoryId: string, change: FileChange): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("unstage_file", { repositoryId, path: change.path, oldPath: change.kind === "renamed" ? change.oldPath : null });
}

export function stageAll(repositoryId: string): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("stage_all", { repositoryId });
}

export function unstageAll(repositoryId: string): Promise<RepositoryStatus> {
  return invoke<RepositoryStatus>("unstage_all", { repositoryId });
}

export function commitStaged(repositoryId: string, message: string): Promise<CommitResult> {
  return invoke<CommitResult>("commit_staged", { repositoryId, message });
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
