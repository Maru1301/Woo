pub mod branches;
pub mod conflicts;
pub mod diff;
mod error;
pub mod git;
pub mod graph;
pub mod history;
pub mod partial_stage;
pub mod remotes;
pub mod repository;
pub mod stash;
pub mod status;
pub mod tags;
pub mod working_tree;

use branches::BranchList;
use conflicts::{ConflictContent, ConflictSide, RepositoryState};
use diff::DiffFile;
use error::AppError;
use history::CommitHistoryPage;
use partial_stage::PartialSelection;
use remotes::RemoteList;
use repository::RepositoryInfo;
use stash::StashList;
use status::{FileChange, RepositoryStatus};
use std::sync::Arc;
use tags::TagList;
use tauri::State;
use working_tree::{
    BranchRefMutationResult, CheckoutResult, CommitResult, ConflictMutationResult,
    HistoryMutationResult, MergeMutationResult, ResetMode, StashMutationResult, TagMutationResult,
    WorkingTree,
};

#[tauri::command]
async fn get_repository_state(
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RepositoryState, AppError> {
    state.repository_state().await
}

#[tauri::command]
async fn get_conflict_content(
    path: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<ConflictContent, AppError> {
    state.conflict_content(&path).await
}

#[tauri::command]
async fn merge_branch(
    full_ref: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<MergeMutationResult, AppError> {
    state.merge_branch(&full_ref).await
}

#[tauri::command]
async fn complete_merge(
    message: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<MergeMutationResult, AppError> {
    state.complete_merge(&message).await
}

#[tauri::command]
async fn abort_merge(state: State<'_, Arc<WorkingTree>>) -> Result<MergeMutationResult, AppError> {
    state.abort_merge().await
}

#[tauri::command]
async fn rebase_onto(
    full_ref: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.rebase_onto(&full_ref).await
}

#[tauri::command]
async fn cherry_pick(
    commit: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.cherry_pick(&commit).await
}

#[tauri::command]
async fn revert_commit(
    commit: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.revert_commit(&commit).await
}

#[tauri::command]
async fn reset_to(
    commit: String,
    mode: ResetMode,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.reset_to(&commit, mode).await
}

#[tauri::command]
async fn continue_history_operation(
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.continue_history_operation().await
}

#[tauri::command]
async fn skip_history_operation(
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.skip_history_operation().await
}

#[tauri::command]
async fn abort_history_operation(
    state: State<'_, Arc<WorkingTree>>,
) -> Result<HistoryMutationResult, AppError> {
    state.abort_history_operation().await
}

#[tauri::command]
async fn save_conflict_text(
    path: String,
    expected: Option<String>,
    text: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<ConflictMutationResult, AppError> {
    state
        .save_conflict_text(&path, expected.as_deref(), &text)
        .await
}

#[tauri::command]
async fn use_conflict_side(
    path: String,
    side: ConflictSide,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<ConflictMutationResult, AppError> {
    state.use_conflict_side(&path, side).await
}

#[tauri::command]
async fn stage_conflict(
    path: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<ConflictMutationResult, AppError> {
    state.stage_conflict(&path).await
}

#[tauri::command]
async fn delete_conflict(
    path: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<ConflictMutationResult, AppError> {
    state.delete_conflict(&path).await
}

#[tauri::command]
async fn rename_branch(
    full_ref: String,
    new_name: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<BranchRefMutationResult, AppError> {
    state.rename_branch(&full_ref, &new_name).await
}

#[tauri::command]
async fn delete_branch(
    full_ref: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<BranchRefMutationResult, AppError> {
    state.delete_branch(&full_ref).await
}

#[tauri::command]
async fn get_stashes(state: State<'_, Arc<WorkingTree>>) -> Result<StashList, AppError> {
    state.stashes().await
}

#[tauri::command]
async fn create_stash(
    message: Option<String>,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<StashMutationResult, AppError> {
    state.create_stash(message.as_deref()).await
}

#[tauri::command]
async fn apply_stash(
    hash: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<StashMutationResult, AppError> {
    state.apply_stash(&hash).await
}

#[tauri::command]
async fn pop_stash(
    hash: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<StashMutationResult, AppError> {
    state.pop_stash(&hash).await
}

#[tauri::command]
async fn drop_stash(
    hash: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<StashMutationResult, AppError> {
    state.drop_stash(&hash).await
}

#[tauri::command]
async fn get_tags(state: State<'_, Arc<WorkingTree>>) -> Result<TagList, AppError> {
    state.tags().await
}

#[tauri::command]
async fn create_tag(
    name: String,
    annotation: Option<String>,
    target_hash: Option<String>,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<TagMutationResult, AppError> {
    state
        .create_tag(&name, annotation.as_deref(), target_hash.as_deref())
        .await
}

#[tauri::command]
async fn delete_tag(
    name: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<TagMutationResult, AppError> {
    state.delete_tag(&name).await
}
use working_tree::{RemoteKind, RemoteOperationStatus};

#[tauri::command]
async fn get_remotes(state: State<'_, Arc<WorkingTree>>) -> Result<RemoteList, AppError> {
    state.remotes().await
}

#[tauri::command]
async fn start_fetch(
    remote: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RemoteOperationStatus, AppError> {
    state
        .inner()
        .start_remote(RemoteKind::Fetch, Some(&remote))
        .await
}

#[tauri::command]
async fn start_pull(state: State<'_, Arc<WorkingTree>>) -> Result<RemoteOperationStatus, AppError> {
    state.inner().start_remote(RemoteKind::Pull, None).await
}

#[tauri::command]
async fn start_push(state: State<'_, Arc<WorkingTree>>) -> Result<RemoteOperationStatus, AppError> {
    state.inner().start_remote(RemoteKind::Push, None).await
}

#[tauri::command]
async fn get_remote_operation(
    id: u64,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RemoteOperationStatus, AppError> {
    state.remote_status(id).await
}

#[tauri::command]
async fn cancel_remote_operation(
    id: u64,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RemoteOperationStatus, AppError> {
    state.cancel_remote(id).await
}

#[tauri::command]
async fn get_branches(state: State<'_, Arc<WorkingTree>>) -> Result<BranchList, AppError> {
    state.branches().await
}

#[tauri::command]
async fn create_branch(
    name: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<BranchList, AppError> {
    state.create_branch(&name).await
}

#[tauri::command]
async fn checkout_branch(
    name: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<CheckoutResult, AppError> {
    state.checkout_branch(&name).await
}

#[tauri::command]
async fn open_repository(
    path: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RepositoryInfo, AppError> {
    state.open(&path).await
}

#[tauri::command]
async fn get_repository_status(
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RepositoryStatus, AppError> {
    state.status().await
}

#[tauri::command]
async fn get_commit_history(
    cursor: Option<String>,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<CommitHistoryPage, AppError> {
    state.history(cursor.as_deref()).await
}

#[tauri::command]
async fn get_unstaged_diff(
    change: FileChange,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<DiffFile, AppError> {
    state.working_diff(false, false, change).await
}

#[tauri::command]
async fn get_staged_diff(
    change: FileChange,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<DiffFile, AppError> {
    state.working_diff(true, false, change).await
}

#[tauri::command]
async fn get_untracked_diff(
    change: FileChange,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<DiffFile, AppError> {
    state.working_diff(false, true, change).await
}

#[tauri::command]
async fn get_commit_files(
    commit: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<Vec<FileChange>, AppError> {
    state.commit_files(&commit).await
}

#[tauri::command]
async fn get_commit_diff(
    commit: String,
    change: FileChange,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<DiffFile, AppError> {
    state.commit_diff(&commit, change).await
}

#[tauri::command]
async fn stage_file(
    path: String,
    old_path: Option<String>,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RepositoryStatus, AppError> {
    state.stage_file(&path, old_path.as_deref()).await
}

#[tauri::command]
async fn unstage_file(
    path: String,
    old_path: Option<String>,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<RepositoryStatus, AppError> {
    state.unstage_file(&path, old_path.as_deref()).await
}

#[tauri::command]
async fn stage_all(state: State<'_, Arc<WorkingTree>>) -> Result<RepositoryStatus, AppError> {
    state.stage_all().await
}

#[tauri::command]
async fn unstage_all(state: State<'_, Arc<WorkingTree>>) -> Result<RepositoryStatus, AppError> {
    state.unstage_all().await
}

#[tauri::command]
async fn partial_stage(
    path: String,
    staged: bool,
    selection: PartialSelection,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<working_tree::PartialStageResult, AppError> {
    state.partial_stage(&path, staged, selection).await
}

#[tauri::command]
async fn commit_staged(
    message: String,
    state: State<'_, Arc<WorkingTree>>,
) -> Result<CommitResult, AppError> {
    state.commit(&message).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(WorkingTree::default()))
        .invoke_handler(tauri::generate_handler![
            get_repository_state,
            get_conflict_content,
            merge_branch,
            complete_merge,
            abort_merge,
            rebase_onto,
            cherry_pick,
            revert_commit,
            reset_to,
            continue_history_operation,
            skip_history_operation,
            abort_history_operation,
            save_conflict_text,
            use_conflict_side,
            stage_conflict,
            delete_conflict,
            rename_branch,
            delete_branch,
            get_stashes,
            create_stash,
            apply_stash,
            pop_stash,
            drop_stash,
            get_tags,
            create_tag,
            delete_tag,
            get_remotes,
            start_fetch,
            start_pull,
            start_push,
            get_remote_operation,
            cancel_remote_operation,
            open_repository,
            get_branches,
            create_branch,
            checkout_branch,
            get_repository_status,
            get_commit_history,
            get_unstaged_diff,
            get_staged_diff,
            get_untracked_diff,
            get_commit_files,
            get_commit_diff,
            stage_file,
            unstage_file,
            stage_all,
            unstage_all,
            partial_stage,
            commit_staged
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Woo");
}
