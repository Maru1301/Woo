pub mod branches;
pub mod diff;
mod error;
pub mod git;
pub mod graph;
pub mod history;
pub mod remotes;
pub mod repository;
pub mod status;
pub mod working_tree;

use branches::BranchList;
use diff::DiffFile;
use error::AppError;
use history::CommitHistoryPage;
use remotes::RemoteList;
use repository::RepositoryInfo;
use status::{FileChange, RepositoryStatus};
use std::sync::Arc;
use tauri::State;
use working_tree::{CheckoutResult, CommitResult, WorkingTree};
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
            commit_staged
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Woo");
}
