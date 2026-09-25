import { invoke } from "@tauri-apps/api/core";

export interface HeadInfo { hash: string; subject: string; authorDate: string }
export interface RepositoryInfo { path: string; branch: string | null; head: HeadInfo | null; openDurationMs: number }
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
export interface ContentPart { text: string | null; isBinary: boolean; oversized: boolean }
export interface ConflictContent { path: string; base: ContentPart | null; ours: ContentPart | null; theirs: ContentPart | null; working: ContentPart | null }
export type MergeOutcome = "already_up_to_date" | "fast_forward" | "clean_merge" | "needs_resolution" | "needs_completion" | "failed" | "completed" | "aborted";
export interface MergeMutationResult { state: RepositoryState; branches: BranchList; branch: string | null; head: HeadInfo; outcome: MergeOutcome; error: AppError | null; resetHistory: boolean; clearDiff: boolean; gitDurationMs: number | null; refreshDurationMs: number }
export interface HistoryMutationResult { state: RepositoryState; branches: BranchList; branch: string | null; head: HeadInfo; error: AppError | null; resetHistory: boolean; clearDiff: boolean; gitDurationMs: number | null; refreshDurationMs: number }
export type ResetMode = "soft" | "mixed" | "hard";
export interface ConflictMutationResult { state: RepositoryState; error: AppError | null }
export interface CommitResult { head: HeadInfo; status: RepositoryStatus }
export type BranchKind = "local" | "remote";
export interface BranchInfo { name: string; fullRefName: string; kind: BranchKind; isCurrent: boolean; targetHash: string; upstream: string | null }
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
export type RemotePhase = "queued" | "running" | "completed" | "failed" | "cancelled";
export interface RemoteOperationStatus { id: number; kind: RemoteKind; phase: RemotePhase; startedAtMs: number; elapsedMs: number; gitDurationMs: number | null; refreshDurationMs: number | null; refresh: RemoteRefresh | null; error: AppError | null }
export interface CommitInfo { hash: string; parentHashes: string[]; authorName: string; authorEmail: string; timestamp: string; subject: string; refs: string[] }
export interface GraphRow { nodeLane: number; laneCount: number; incoming: boolean; continuations: number[]; parentLanes: number[] }
export interface CommitHistoryPage { commits: CommitInfo[]; graphRows: GraphRow[]; nextCursor: string | null; hasMore: boolean }
export type DiffLineKind = "context" | "addition" | "deletion" | "no_newline";
export interface DiffLine { kind: DiffLineKind; oldLineNumber: number | null; newLineNumber: number | null; content: string }
export interface DiffHunk { oldStart: number; oldCount: number; newStart: number; newCount: number; header: string; lines: DiffLine[] }
export interface DiffFile { change: FileChange; isBinary: boolean; hunks: DiffHunk[]; revision: string; partialStageable: boolean }
export interface PartialSelection { revision: string; hunkIndex: number; lineIndices: number[] | null }
export interface PartialStageResult { status: RepositoryStatus; stagedDiff: DiffFile | null; unstagedDiff: DiffFile | null; error: AppError | null }

export function getRepositoryState(): Promise<RepositoryState> { return invoke<RepositoryState>("get_repository_state"); }
export function getConflictContent(path: string): Promise<ConflictContent> { return invoke<ConflictContent>("get_conflict_content", { path }); }
export function mergeBranch(fullRef: string): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("merge_branch", { fullRef }); }
export function completeMerge(message: string): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("complete_merge", { message }); }
export function abortMerge(): Promise<MergeMutationResult> { return invoke<MergeMutationResult>("abort_merge"); }
export function rebaseOnto(fullRef: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("rebase_onto", { fullRef }); }
export function cherryPick(commit: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("cherry_pick", { commit }); }
export function revertCommit(commit: string): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("revert_commit", { commit }); }
export function resetTo(commit: string, mode: ResetMode): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("reset_to", { commit, mode }); }
export function continueHistoryOperation(): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("continue_history_operation"); }
export function skipHistoryOperation(): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("skip_history_operation"); }
export function abortHistoryOperation(): Promise<HistoryMutationResult> { return invoke<HistoryMutationResult>("abort_history_operation"); }
export function saveConflictText(path: string, expected: string | null, text: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("save_conflict_text", { path, expected, text }); }
export function useConflictSide(path: string, side: "ours" | "theirs"): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("use_conflict_side", { path, side }); }
export function stageConflict(path: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("stage_conflict", { path }); }
export function deleteConflict(path: string): Promise<ConflictMutationResult> { return invoke<ConflictMutationResult>("delete_conflict", { path }); }

export function getUnstagedDiff(change: FileChange): Promise<DiffFile> { return invoke<DiffFile>("get_unstaged_diff", { change }); }
export function partialStage(path: string, staged: boolean, selection: PartialSelection): Promise<PartialStageResult> { return invoke<PartialStageResult>("partial_stage", { path, staged, selection }); }
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
export function renameBranch(fullRef: string, newName: string): Promise<BranchRefMutationResult> { return invoke<BranchRefMutationResult>("rename_branch", { fullRef, newName }); }
export function deleteBranch(fullRef: string): Promise<BranchRefMutationResult> { return invoke<BranchRefMutationResult>("delete_branch", { fullRef }); }
export function getStashes(): Promise<StashList> { return invoke<StashList>("get_stashes"); }
export function createStash(message: string | null): Promise<StashMutationResult> { return invoke<StashMutationResult>("create_stash", { message }); }
export function applyStash(hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("apply_stash", { hash }); }
export function popStash(hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("pop_stash", { hash }); }
export function dropStash(hash: string): Promise<StashMutationResult> { return invoke<StashMutationResult>("drop_stash", { hash }); }
export function getTags(): Promise<TagList> { return invoke<TagList>("get_tags"); }
export function createTag(name: string, annotation: string | null, targetHash: string | null): Promise<TagMutationResult> { return invoke<TagMutationResult>("create_tag", { name, annotation, targetHash }); }
export function deleteTag(name: string): Promise<TagMutationResult> { return invoke<TagMutationResult>("delete_tag", { name }); }
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
