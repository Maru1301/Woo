use crate::{
    branches::{self, BranchList},
    conflicts::{
        self, ConflictContent, ConflictFile, ConflictSide, RepositoryOperationState,
        RepositoryState,
    },
    diff::{self, DiffFile},
    error::{git_failure, AppError},
    git::{GitRunError, GitRunner},
    history::{self, CommitHistoryPage},
    operation_log::{OperationLog, OperationOutcome, OperationSource},
    partial_stage::{self, PartialSelection},
    remotes::{self, RemoteList},
    repository::{self, HeadInfo, RepositoryInfo},
    stash::{self, StashList},
    status::{parse_status, FileChange, RepositoryStatus},
    tags::{self, TagList},
};
use serde::Serialize;
use std::{
    future::Future,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, watch, Mutex};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKind {
    Fetch,
    Pull,
    Push,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemotePhase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRefresh {
    pub branches: BranchList,
    pub head: Option<HeadInfo>,
    pub status: Option<RepositoryStatus>,
    pub operation: Option<RepositoryOperationState>,
    pub conflicts: Option<Vec<ConflictFile>>,
    pub reset_history: bool,
    pub refresh_history: bool,
    pub clear_diff: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteOperationStatus {
    pub id: u64,
    pub session_id: u64,
    pub source: OperationSource,
    pub kind: RemoteKind,
    pub phase: RemotePhase,
    pub started_at_ms: u128,
    pub elapsed_ms: u128,
    pub git_duration_ms: Option<u128>,
    pub refresh_duration_ms: Option<u128>,
    pub refresh: Option<RemoteRefresh>,
    pub error: Option<AppError>,
}

struct RemoteTask {
    status: RemoteOperationStatus,
    started: Instant,
    cancel: watch::Sender<bool>,
}

struct RemoteCompletion {
    refresh: RemoteRefresh,
    git_ms: Option<u128>,
    refresh_ms: u128,
    error: Option<AppError>,
}

#[derive(Clone, Copy)]
enum StashAction {
    Apply,
    Pop,
    Drop,
}

#[derive(Debug)]
pub struct StatusTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
}

#[derive(Debug, Serialize)]
pub struct CommitResult {
    pub head: HeadInfo,
    pub status: RepositoryStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialStageResult {
    pub status: RepositoryStatus,
    pub staged_diff: Option<DiffFile>,
    pub unstaged_diff: Option<DiffFile>,
    pub error: Option<AppError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResult {
    pub branch: Option<String>,
    pub head: HeadInfo,
    pub status: RepositoryStatus,
    pub branches: BranchList,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchRefMutationResult {
    pub branch: Option<String>,
    pub branches: BranchList,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashMutationResult {
    pub stashes: StashList,
    pub status: Option<RepositoryStatus>,
    pub operation: Option<RepositoryOperationState>,
    pub conflicts: Option<Vec<ConflictFile>>,
    pub reset_history: bool,
    pub clear_diff: bool,
    pub error: Option<AppError>,
}

#[derive(Debug, Serialize)]
pub struct TagMutationResult {
    pub tags: TagList,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeOutcome {
    AlreadyUpToDate,
    FastForward,
    CleanMerge,
    NeedsResolution,
    NeedsCompletion,
    Failed,
    Completed,
    Aborted,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeMutationResult {
    pub state: RepositoryState,
    pub branches: BranchList,
    pub branch: Option<String>,
    pub head: HeadInfo,
    pub outcome: MergeOutcome,
    pub error: Option<AppError>,
    pub reset_history: bool,
    pub clear_diff: bool,
    pub git_duration_ms: Option<u128>,
    pub refresh_duration_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictMutationResult {
    pub state: RepositoryState,
    pub error: Option<AppError>,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMutationResult {
    pub state: RepositoryState,
    pub branches: BranchList,
    pub branch: Option<String>,
    pub head: HeadInfo,
    pub error: Option<AppError>,
    pub reset_history: bool,
    pub clear_diff: bool,
    pub git_duration_ms: Option<u128>,
    pub refresh_duration_ms: u128,
}

enum HistoryCommand<'a> {
    Rebase(&'a str),
    CherryPick(&'a str),
    Revert(&'a str),
    Reset(&'a str, ResetMode),
    Continue,
    Skip,
    Abort,
}

/// One Git process supplies the complete working-tree status.
pub async fn load_status(
    git: &GitRunner,
    repository: &Path,
) -> Result<(RepositoryStatus, StatusTiming), AppError> {
    let started = Instant::now();
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let output = git
        .run(
            repository,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--renames",
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let status = parse_status(&output.stdout)?;
    let timing = StatusTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
    };
    eprintln!(
        "Git.Status git_ms={} parse_us={} total_ms={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis()
    );
    Ok((status, timing))
}

async fn load_repository_state(
    git: &GitRunner,
    repository: &Path,
) -> Result<RepositoryState, AppError> {
    let (status, _) = load_status(git, repository).await?;
    conflicts::load_state(git, repository, status).await
}

async fn refresh_partial(
    git: &GitRunner,
    repository: &Path,
    path: &str,
    error: Option<AppError>,
) -> Result<PartialStageResult, AppError> {
    let (status, _) = load_status(git, repository).await.map_err(|cause| {
        AppError::new(
            "partial_refresh_failed",
            format!(
                "Index state may have changed, but refresh failed: {}",
                cause.message
            ),
        )
    })?;
    let mut error = error;
    let staged_diff = if let Some(change) = status.staged.iter().find(|change| change.path == path)
    {
        match diff::load_working_diff(git, repository, true, false, change.clone()).await {
            Ok((file, _)) => Some(file),
            Err(cause) => {
                error = Some(AppError::new(
                    "diff_refresh_failed",
                    format!(
                        "Index status refreshed, but the file diff could not be loaded: {}",
                        cause.message
                    ),
                ));
                None
            }
        }
    } else {
        None
    };
    let unstaged_diff =
        if let Some(change) = status.unstaged.iter().find(|change| change.path == path) {
            match diff::load_working_diff(git, repository, false, false, change.clone()).await {
                Ok((file, _)) => Some(file),
                Err(cause) => {
                    error = Some(AppError::new(
                        "diff_refresh_failed",
                        format!(
                            "Index status refreshed, but the file diff could not be loaded: {}",
                            cause.message
                        ),
                    ));
                    None
                }
            }
        } else {
            None
        };
    Ok(PartialStageResult {
        status,
        staged_diff,
        unstaged_diff,
        error,
    })
}

async fn require_no_operation(git: &GitRunner, repository: &Path) -> Result<(), AppError> {
    match conflicts::operation_state(git, repository).await? {
        RepositoryOperationState::None => Ok(()),
        RepositoryOperationState::Merge { .. } => Err(AppError::new(
            "merge_in_progress",
            "Finish or abort the current merge first.",
        )),
        _ => Err(AppError::new(
            "operation_in_progress",
            "Continue or abort the current repository operation first.",
        )),
    }
}

fn require_idle(state: &RepositoryState) -> Result<(), AppError> {
    if !matches!(state.operation, RepositoryOperationState::None) || !state.conflicts.is_empty() {
        Err(AppError::new(
            "operation_in_progress",
            "Resolve or abort the current repository operation first.",
        ))
    } else {
        Ok(())
    }
}

fn operation_command(state: &RepositoryOperationState) -> Result<&'static str, AppError> {
    match state {
        RepositoryOperationState::Rebase { .. } => Ok("rebase"),
        RepositoryOperationState::CherryPick { .. } => Ok("cherry-pick"),
        RepositoryOperationState::Revert { .. } => Ok("revert"),
        _ => Err(AppError::new(
            "operation_not_supported",
            "There is no rebase, cherry-pick, or revert to continue or abort.",
        )),
    }
}

async fn require_current_branch(git: &GitRunner, repository: &Path) -> Result<(), AppError> {
    let output = git
        .run(repository, &["symbolic-ref", "-q", "HEAD"])
        .await
        .map_err(AppError::from)?;
    if output.success() && output.stdout_text().starts_with("refs/heads/") {
        Ok(())
    } else {
        Err(AppError::new(
            "detached_head",
            "Check out a local branch before changing its history.",
        ))
    }
}

async fn reject_merge_commit(
    git: &GitRunner,
    repository: &Path,
    hash: &str,
) -> Result<(), AppError> {
    let output = git
        .run(repository, &["rev-list", "--parents", "-n", "1", hash])
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    if output.stdout_text().split_whitespace().count() > 2 {
        return Err(AppError::new(
            "merge_commit_unsupported",
            "Choose a non-merge commit; mainline selection is not available yet.",
        ));
    }
    Ok(())
}

async fn protect_untracked(git: &GitRunner, repository: &Path, hash: &str) -> Result<(), AppError> {
    // Include ignored files: Git may remove an obstructing ignored path too.
    let others = git
        .run_limited(repository, &["ls-files", "--others", "-z"], 8 * 1024 * 1024)
        .await
        .map_err(AppError::from)?;
    if !others.success() {
        return Err(git_failure(&others));
    }
    let untracked: Vec<&str> = others
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path).map_err(|_| {
                AppError::new(
                    "invalid_path_encoding",
                    "An untracked path cannot be compared safely.",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    if untracked.is_empty() {
        return Ok(());
    }
    let output = git
        .run_limited(
            repository,
            &["ls-tree", "-r", "-z", "--name-only", hash],
            8 * 1024 * 1024,
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    for path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let target = std::str::from_utf8(path).map_err(|_| {
            AppError::new(
                "invalid_path_encoding",
                "Target commit contains a path Woo cannot safely compare.",
            )
        })?;
        if untracked.iter().any(|file| {
            *file == target
                || file
                    .strip_prefix(target)
                    .is_some_and(|rest| rest.starts_with('/'))
                || target
                    .strip_prefix(file)
                    .is_some_and(|rest| rest.starts_with('/'))
        }) {
            return Err(AppError::new(
                "untracked_would_be_overwritten",
                "Hard reset could overwrite an untracked path. Move it out of the way first.",
            ));
        }
    }
    Ok(())
}

pub(crate) fn valid_relative_path(path: &str) -> Result<(), AppError> {
    if path.is_empty()
        || path.as_bytes().contains(&0)
        || !Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(AppError::new(
            "invalid_path",
            "Choose a file inside the open repository.",
        ));
    }
    Ok(())
}

/// One repository session. Its mutex keeps reads and mutation-plus-refresh
/// consistent; the shared mutation gate serializes mutations across sessions.
pub struct WorkingTree {
    git: GitRunner,
    repository_id: std::sync::RwLock<Option<String>>,
    mutation_gate: Arc<Mutex<()>>,
    repository: Mutex<Option<PathBuf>>,
    generation: AtomicU64,
    session_id: AtomicU64,
    remote_task: Mutex<Option<RemoteTask>>,
    operation_log: OperationLog,
    remote_completions: broadcast::Sender<RemoteOperationStatus>,
    active_user_operations: AtomicU64,
}

struct UserOperationGuard<'a>(&'a AtomicU64);
impl Drop for UserOperationGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for WorkingTree {
    fn default() -> Self {
        Self {
            git: GitRunner::with_timeout(Duration::from_secs(60)),
            repository_id: std::sync::RwLock::new(None),
            mutation_gate: Arc::new(Mutex::new(())),
            repository: Mutex::new(None),
            generation: AtomicU64::new(0),
            session_id: AtomicU64::new(0),
            remote_task: Mutex::new(None),
            operation_log: OperationLog::default(),
            remote_completions: broadcast::channel(16).0,
            active_user_operations: AtomicU64::new(0),
        }
    }
}

impl WorkingTree {
    pub fn with_shared_mutation_gate(mutation_gate: Arc<Mutex<()>>) -> Self {
        Self {
            mutation_gate,
            ..Self::default()
        }
    }

    pub fn with_mutation_gate(repository_id: String, mutation_gate: Arc<Mutex<()>>) -> Self {
        Self {
            repository_id: std::sync::RwLock::new(Some(repository_id)),
            mutation_gate,
            ..Self::default()
        }
    }

    pub fn set_repository_id(&self, repository_id: Option<String>) {
        *self
            .repository_id
            .write()
            .expect("repository identity lock") = repository_id;
    }

    pub fn repository_id(&self) -> Option<String> {
        self.repository_id
            .read()
            .expect("repository identity lock")
            .clone()
    }

    fn operation_repository_id(&self, path: &Path) -> String {
        self.repository_id
            .read()
            .expect("repository identity lock")
            .clone()
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }

    pub fn active_session_id(&self) -> u64 {
        self.session_id.load(Ordering::SeqCst)
    }

    pub async fn info(&self) -> Result<RepositoryInfo, AppError> {
        let (path, generation) = self.snapshot().await?;
        let (branch, head) = repository::read_identity(&self.git, &path).await?;
        self.ensure_generation(generation)?;
        Ok(RepositoryInfo {
            path: path.to_string_lossy().into_owned(),
            branch,
            head,
            open_duration_ms: 0,
            session_id: self.active_session_id(),
            watch_warning: None,
        })
    }
    async fn snapshot(&self) -> Result<(PathBuf, u64), AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_ref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?
            .clone();
        Ok((path, self.generation.load(Ordering::SeqCst)))
    }

    fn ensure_generation(&self, generation: u64) -> Result<(), AppError> {
        if self.generation.load(Ordering::SeqCst) != generation {
            Err(AppError::new(
                "repository_changed",
                "The repository changed while data was loading.",
            ))
        } else {
            Ok(())
        }
    }

    pub async fn open(&self, path: &str) -> Result<RepositoryInfo, AppError> {
        // Opening a new session requests cancellation before waiting for the
        // mutation lock held by a remote process.
        self.cancel_active_remote().await;
        let mut current = self.repository.lock().await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.session_id.store(
            NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst) + 1,
            Ordering::SeqCst,
        );
        *current = None;
        let mut info = repository::open(&self.git, path).await?;
        info.session_id = self.session_id.load(Ordering::SeqCst);
        *current = Some(PathBuf::from(&info.path));
        Ok(info)
    }

    pub async fn close(&self) {
        self.cancel_active_remote().await;
        let mut current = self.repository.lock().await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.session_id.store(
            NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst) + 1,
            Ordering::SeqCst,
        );
        *current = None;
    }

    pub async fn watch_snapshot(
        &self,
        session_id: u64,
        read_state: bool,
        read_identity: bool,
        read_branches: bool,
        previous_head: Option<&str>,
    ) -> Result<crate::watcher::ValidatedState, AppError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self.repository.lock().await;
        if self.session_id.load(Ordering::SeqCst) != session_id {
            return Err(AppError::new(
                "repository_changed",
                "The repository session changed.",
            ));
        }
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_unavailable",
                "The repository is unavailable.",
            ));
        }
        let identity = if read_identity {
            Some(repository::read_identity(&self.git, path).await?)
        } else {
            None
        };
        let head_changed = identity
            .as_ref()
            .is_some_and(|(_, head)| head.as_ref().map(|head| head.hash.as_str()) != previous_head);
        let state = if read_state || head_changed {
            Some(load_repository_state(&self.git, path).await?)
        } else {
            None
        };
        let branches = if read_branches {
            Some(branches::load_branches(&self.git, path).await?.0)
        } else {
            None
        };
        Ok(crate::watcher::ValidatedState {
            identity,
            state,
            branches,
        })
    }

    pub async fn remotes(&self) -> Result<RemoteList, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = remotes::load_remotes(&self.git, &path).await;
        self.ensure_generation(generation)?;
        result.map(|(list, _)| list)
    }

    async fn cancel_active_remote(&self) {
        let task = self.remote_task.lock().await;
        if let Some(task) = task.as_ref() {
            if matches!(
                task.status.phase,
                RemotePhase::Queued | RemotePhase::Running
            ) {
                let _ = task.cancel.send(true);
            }
        }
    }

    pub async fn cancel_background_remote(&self) {
        let task = self.remote_task.lock().await;
        if let Some(task) = task
            .as_ref()
            .filter(|task| task.status.source == OperationSource::Background)
        {
            if matches!(
                task.status.phase,
                RemotePhase::Queued | RemotePhase::Running
            ) {
                let _ = task.cancel.send(true);
            }
        }
    }

    pub async fn operation_history(&self) -> Vec<crate::operation_log::OperationEntry> {
        self.operation_log.list().await
    }

    pub fn remote_completions(&self) -> broadcast::Receiver<RemoteOperationStatus> {
        self.remote_completions.subscribe()
    }

    pub async fn logged_user<T, F>(&self, kind: &str, future: F) -> Result<T, AppError>
    where
        T: OperationOutcome,
        F: Future<Output = Result<T, AppError>>,
    {
        self.cancel_background_remote().await;
        let _mutation = self.mutation_gate.lock().await;
        let (path, _) = self.snapshot().await?;
        let id = NEXT_OPERATION_ID.fetch_add(1, Ordering::SeqCst) + 1;
        let started = Instant::now();
        self.active_user_operations.fetch_add(1, Ordering::SeqCst);
        let _guard = UserOperationGuard(&self.active_user_operations);
        self.operation_log
            .begin(
                id,
                self.operation_repository_id(&path),
                kind,
                OperationSource::User,
            )
            .await;
        let result = future.await;
        let error = match &result {
            Ok(value) => value.semantic_error(),
            Err(error) => Some(error),
        };
        self.operation_log.finish(id, started, error).await;
        result
    }

    pub async fn start_remote(
        self: &Arc<Self>,
        kind: RemoteKind,
        remote: Option<&str>,
    ) -> Result<RemoteOperationStatus, AppError> {
        let mut completed = self.remote_completions();
        let background_id = self
            .remote_task
            .lock()
            .await
            .as_ref()
            .filter(|task| {
                task.status.source == OperationSource::Background
                    && matches!(
                        task.status.phase,
                        RemotePhase::Queued | RemotePhase::Running
                    )
            })
            .map(|task| task.status.id);
        self.cancel_background_remote().await;
        if let Some(background_id) = background_id {
            let stopped = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match completed.recv().await {
                        Ok(status) if status.id == background_id => break,
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            })
            .await;
            if stopped.is_err() {
                return Err(AppError::new(
                    "operation_busy",
                    "Background fetch is still stopping. Try again shortly.",
                ));
            }
        }
        self.start_remote_with_source(kind, remote, OperationSource::User)
            .await
    }

    pub async fn start_background_fetch(
        self: &Arc<Self>,
        expected_session: u64,
    ) -> Result<Option<RemoteOperationStatus>, AppError> {
        if self.active_user_operations.load(Ordering::SeqCst) > 0 {
            return Ok(None);
        }
        let current = match self.repository.try_lock() {
            Ok(current) => current,
            Err(_) => return Ok(None),
        };
        if self.session_id.load(Ordering::SeqCst) != expected_session {
            return Ok(None);
        }
        let Some(path) = current.as_ref() else {
            return Ok(None);
        };
        let slot = match self.remote_task.try_lock() {
            Ok(slot) => slot,
            Err(_) => return Ok(None),
        };
        if slot.as_ref().is_some_and(|task| {
            matches!(
                task.status.phase,
                RemotePhase::Queued | RemotePhase::Running
            )
        }) {
            return Ok(None);
        }
        drop(slot);
        if !matches!(
            conflicts::operation_state(&self.git, path).await?,
            RepositoryOperationState::None
        ) {
            return Ok(None);
        }
        let (configured, _) = remotes::load_remotes(&self.git, path).await?;
        if configured.remotes.is_empty() {
            return Ok(None);
        }
        drop(current);
        self.start_remote_with_source(RemoteKind::Fetch, None, OperationSource::Background)
            .await
            .map(Some)
    }

    async fn start_remote_with_source(
        self: &Arc<Self>,
        kind: RemoteKind,
        remote: Option<&str>,
        source: OperationSource,
    ) -> Result<RemoteOperationStatus, AppError> {
        let (path, generation) = self.snapshot().await?;
        let remote = if matches!(kind, RemoteKind::Fetch) && source == OperationSource::User {
            let name = remote
                .ok_or_else(|| AppError::new("remote_missing", "Choose a remote to fetch."))?;
            let (configured, _) = remotes::load_remotes(&self.git, &path).await?;
            if !configured.remotes.iter().any(|item| item.name == name) {
                return Err(AppError::new(
                    "remote_missing",
                    "The selected remote is no longer configured.",
                ));
            }
            Some(name.to_owned())
        } else {
            None
        };
        self.ensure_generation(generation)?;
        let mut slot = self.remote_task.lock().await;
        if slot.as_ref().is_some_and(|task| {
            matches!(
                task.status.phase,
                RemotePhase::Queued | RemotePhase::Running
            )
        }) {
            return Err(AppError::new(
                "operation_busy",
                "Another remote operation is already running.",
            ));
        }
        let id = NEXT_OPERATION_ID.fetch_add(1, Ordering::SeqCst) + 1;
        let (cancel, receiver) = watch::channel(false);
        let status = RemoteOperationStatus {
            id,
            session_id: self.session_id.load(Ordering::SeqCst),
            source,
            kind,
            phase: RemotePhase::Queued,
            started_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            elapsed_ms: 0,
            git_duration_ms: None,
            refresh_duration_ms: None,
            refresh: None,
            error: None,
        };
        *slot = Some(RemoteTask {
            status: status.clone(),
            started: Instant::now(),
            cancel,
        });
        drop(slot);
        self.operation_log
            .begin(
                id,
                self.operation_repository_id(&path),
                match kind {
                    RemoteKind::Fetch => "Fetch",
                    RemoteKind::Pull => "Pull",
                    RemoteKind::Push => "Push",
                },
                source,
            )
            .await;
        let owner = Arc::clone(self);
        tokio::spawn(async move {
            owner
                .run_remote_task(id, generation, kind, remote, receiver)
                .await;
        });
        Ok(status)
    }

    pub async fn remote_status(&self, id: u64) -> Result<RemoteOperationStatus, AppError> {
        let slot = self.remote_task.lock().await;
        let task = slot
            .as_ref()
            .filter(|task| task.status.id == id)
            .ok_or_else(|| {
                AppError::new(
                    "operation_missing",
                    "The remote operation is no longer available.",
                )
            })?;
        let mut status = task.status.clone();
        if matches!(status.phase, RemotePhase::Queued | RemotePhase::Running) {
            status.elapsed_ms = task.started.elapsed().as_millis();
        }
        Ok(status)
    }

    pub async fn cancel_remote(&self, id: u64) -> Result<RemoteOperationStatus, AppError> {
        let slot = self.remote_task.lock().await;
        let task = slot
            .as_ref()
            .filter(|task| task.status.id == id)
            .ok_or_else(|| {
                AppError::new(
                    "operation_missing",
                    "The remote operation is no longer available.",
                )
            })?;
        if matches!(
            task.status.phase,
            RemotePhase::Queued | RemotePhase::Running
        ) {
            let _ = task.cancel.send(true);
        }
        Ok(task.status.clone())
    }

    async fn update_remote(
        &self,
        id: u64,
        update: impl FnOnce(&mut RemoteOperationStatus),
    ) -> Option<RemoteOperationStatus> {
        let mut slot = self.remote_task.lock().await;
        if let Some(task) = slot.as_mut().filter(|task| task.status.id == id) {
            update(&mut task.status);
            task.status.elapsed_ms = task.started.elapsed().as_millis();
            return Some(task.status.clone());
        }
        None
    }

    async fn run_remote_task(
        self: Arc<Self>,
        id: u64,
        generation: u64,
        kind: RemoteKind,
        remote: Option<String>,
        cancelled: watch::Receiver<bool>,
    ) {
        let _mutation = self.mutation_gate.lock().await;
        let started = Instant::now();
        let source = self
            .remote_status(id)
            .await
            .map(|status| status.source)
            .unwrap_or(OperationSource::User);
        let outcome = self
            .execute_remote(id, generation, kind, remote.as_deref(), cancelled, source)
            .await;
        let log_error = match &outcome {
            Ok(completion) => completion.error.clone(),
            Err(error) => Some(error.clone()),
        };
        self.operation_log
            .finish(id, started, log_error.as_ref())
            .await;
        let completed = self
            .update_remote(id, |status| match outcome {
                Ok(completion) => {
                    status.phase = match completion.error.as_ref().map(|error| error.code) {
                        Some("operation_cancelled") => RemotePhase::Cancelled,
                        Some("git_timeout") => RemotePhase::TimedOut,
                        Some(_) => RemotePhase::Failed,
                        None => RemotePhase::Completed,
                    };
                    status.git_duration_ms = completion.git_ms;
                    status.refresh_duration_ms = Some(completion.refresh_ms);
                    status.refresh = Some(completion.refresh);
                    status.error = completion.error;
                }
                Err(error) => {
                    status.phase = if error.code == "operation_cancelled" {
                        RemotePhase::Cancelled
                    } else if error.code == "git_timeout" {
                        RemotePhase::TimedOut
                    } else {
                        RemotePhase::Failed
                    };
                    status.error = Some(error);
                }
            })
            .await;
        if let Some(status) = completed {
            let _ = self.remote_completions.send(status);
        }
    }

    async fn execute_remote(
        &self,
        id: u64,
        generation: u64,
        kind: RemoteKind,
        remote: Option<&str>,
        cancelled: watch::Receiver<bool>,
        source: OperationSource,
    ) -> Result<RemoteCompletion, AppError> {
        let current = if source == OperationSource::Background {
            self.repository.try_lock().map_err(|_| {
                AppError::new(
                    "operation_busy",
                    "User operation is active; background fetch skipped.",
                )
            })?
        } else {
            self.repository.lock().await
        };
        self.ensure_generation(generation)?;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        if matches!(kind, RemoteKind::Pull) {
            require_no_operation(&self.git, path).await?;
        }
        self.update_remote(id, |status| status.phase = RemotePhase::Running)
            .await;
        let args: Vec<&str> = match kind {
            RemoteKind::Fetch => {
                if let Some(remote) = remote {
                    vec!["fetch", "--", remote]
                } else {
                    vec!["fetch", "--all"]
                }
            }
            RemoteKind::Pull => vec!["pull"],
            RemoteKind::Push => vec!["push"],
        };
        let process = self
            .git
            .run_remote(
                path,
                &args,
                cancelled,
                (source == OperationSource::Background).then_some(Duration::from_secs(120)),
            )
            .await;
        let (git_ms, operation_error) = match process {
            Ok(output) => {
                let error = (!output.success()).then(|| remotes::remote_failure(&output));
                (Some(output.duration.as_millis()), error)
            }
            Err(error) => (None, Some(AppError::from(error))),
        };
        if matches!(kind, RemoteKind::Pull) {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        let refresh_start = Instant::now();
        let (branches, _) = branches::load_branches(&self.git, path)
            .await
            .map_err(|error| {
                AppError::new(
                    "remote_refresh_failed",
                    format!(
                        "Remote operation succeeded, but branches could not be refreshed: {}",
                        error.message
                    ),
                )
            })?;
        let (head, status, operation, conflicts) = if matches!(kind, RemoteKind::Pull) {
            let (head, _) = repository::read_head(&self.git, path)
                .await
                .map_err(|error| {
                    AppError::new(
                        "remote_refresh_failed",
                        format!(
                            "Pull succeeded, but HEAD could not be refreshed: {}",
                            error.message
                        ),
                    )
                })?;
            let (status, _) = load_status(&self.git, path).await.map_err(|error| {
                AppError::new(
                    "remote_refresh_failed",
                    format!(
                        "Pull succeeded, but status could not be refreshed: {}",
                        error.message
                    ),
                )
            })?;
            let state = conflicts::load_state(&self.git, path, status)
                .await
                .map_err(|error| {
                    AppError::new(
                        "remote_refresh_failed",
                        format!(
                            "Pull ran, but operation state could not be refreshed: {}",
                            error.message
                        ),
                    )
                })?;
            (
                Some(head),
                Some(state.status),
                Some(state.operation),
                Some(state.conflicts),
            )
        } else {
            (None, None, None, None)
        };
        let refresh_ms = refresh_start.elapsed().as_millis();
        eprintln!(
            "Git.Remote kind={kind:?} git_ms={git_ms:?} refresh_ms={} processes={}",
            refresh_ms,
            if matches!(kind, RemoteKind::Pull) {
                4
            } else {
                2
            }
        );
        Ok(RemoteCompletion {
            refresh: RemoteRefresh {
                branches,
                head,
                status,
                operation,
                conflicts,
                reset_history: matches!(kind, RemoteKind::Pull),
                refresh_history: matches!(kind, RemoteKind::Fetch | RemoteKind::Push),
                clear_diff: matches!(kind, RemoteKind::Pull),
            },
            git_ms,
            refresh_ms,
            error: operation_error,
        })
    }

    pub async fn history(&self, cursor: Option<&str>) -> Result<CommitHistoryPage, AppError> {
        let (path, generation) = {
            let current = self.repository.lock().await;
            let path = current
                .as_ref()
                .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?
                .clone();
            (path, self.generation.load(Ordering::SeqCst))
        };
        let result = history::load_history(&self.git, &path, cursor).await;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(AppError::new(
                "history_stale",
                "The repository changed while history was loading.",
            ));
        }
        result.map(|(page, _)| page)
    }

    pub async fn working_diff(
        &self,
        staged: bool,
        untracked: bool,
        change: FileChange,
    ) -> Result<DiffFile, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = diff::load_working_diff(&self.git, &path, staged, untracked, change).await;
        self.ensure_generation(generation)?;
        result.map(|(file, _)| file)
    }

    pub async fn commit_files(&self, commit: &str) -> Result<Vec<FileChange>, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = diff::load_commit_files(&self.git, &path, commit).await;
        self.ensure_generation(generation)?;
        result
    }

    pub async fn commit_diff(
        &self,
        commit: &str,
        change: FileChange,
    ) -> Result<DiffFile, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = diff::load_commit_diff(&self.git, &path, commit, change).await;
        self.ensure_generation(generation)?;
        result.map(|(file, _)| file)
    }

    pub async fn status(&self) -> Result<RepositoryStatus, AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let (status, _) = load_status(&self.git, path).await?;
        Ok(status)
    }

    pub async fn repository_state(&self) -> Result<RepositoryState, AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        load_repository_state(&self.git, path).await
    }

    pub async fn conflict_content(&self, path: &str) -> Result<ConflictContent, AppError> {
        valid_relative_path(path)?;
        let (repository, generation) = self.snapshot().await?;
        let files = conflicts::list_conflicts(&self.git, &repository).await?;
        let file = conflicts::selected(&files, path)?;
        let result = conflicts::load_content(&self.git, &repository, file).await;
        self.ensure_generation(generation)?;
        result
    }

    pub async fn merge_branch(&self, full_ref: &str) -> Result<MergeMutationResult, AppError> {
        branches::local_name(full_ref)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let before = load_repository_state(&self.git, path).await?;
        if matches!(before.operation, RepositoryOperationState::Merge { .. }) {
            return Err(AppError::new(
                "merge_in_progress",
                "Finish or abort the current merge first.",
            ));
        }
        if !matches!(before.operation, RepositoryOperationState::None) {
            return Err(AppError::new(
                "operation_in_progress",
                "Continue or abort the current repository operation first.",
            ));
        }
        if !before.conflicts.is_empty() {
            return Err(AppError::new(
                "unresolved_conflicts",
                "Resolve the existing conflicts before starting a merge.",
            ));
        }
        let (listed, _) = branches::load_branches(&self.git, path).await?;
        let target = listed
            .branches
            .iter()
            .find(|branch| {
                branch.full_ref_name == full_ref
                    && matches!(branch.kind, branches::BranchKind::Local)
            })
            .ok_or_else(|| AppError::new("branch_missing", "Choose an existing local branch."))?;
        let current_branch = listed
            .branches
            .iter()
            .find(|branch| branch.is_current)
            .ok_or_else(|| {
                AppError::new("detached_head", "Check out a local branch before merging.")
            })?;
        if current_branch.full_ref_name == full_ref {
            return Err(AppError::new(
                "invalid_merge_target",
                "Choose a different local branch to merge.",
            ));
        }
        let before_hash = current_branch.target_hash.clone();
        let target_hash = target.target_hash.clone();
        let runner = GitRunner::with_timeout(Duration::from_secs(300));
        let process = runner
            .run_limited(path, &["merge", "--no-edit", "--", full_ref], 1024 * 1024)
            .await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let (git_ms, error) = match process {
            Ok(output) => (
                Some(output.duration.as_millis()),
                (!output.success()).then(|| git_failure(&output)),
            ),
            Err(GitRunError::OutputLimit) => (
                None,
                Some(AppError::new("merge_output_too_large", "Merge output exceeded the safe capture limit. Inspect the refreshed merge state.")),
            ),
            Err(cause) => (None, Some(AppError::from(cause))),
        };
        let mut result = self
            .refresh_merge(path, &before_hash, error, git_ms)
            .await?;
        result.outcome = match &result.state.operation {
            RepositoryOperationState::Merge { .. } if !result.state.conflicts.is_empty() => {
                MergeOutcome::NeedsResolution
            }
            RepositoryOperationState::Merge { .. } => MergeOutcome::NeedsCompletion,
            RepositoryOperationState::None
                if result.head.hash == before_hash && result.error.is_none() =>
            {
                MergeOutcome::AlreadyUpToDate
            }
            RepositoryOperationState::None if result.head.hash == target_hash => {
                MergeOutcome::FastForward
            }
            RepositoryOperationState::None if result.reset_history => MergeOutcome::CleanMerge,
            _ => MergeOutcome::Failed,
        };
        Ok(result)
    }

    pub async fn complete_merge(&self, message: &str) -> Result<MergeMutationResult, AppError> {
        if message.trim().is_empty() || message.len() > 1024 * 1024 || message.contains('\0') {
            return Err(AppError::new(
                "invalid_merge_message",
                "Enter a merge message under 1 MiB without binary data.",
            ));
        }
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !matches!(
            conflicts::operation_state(&self.git, path).await?,
            RepositoryOperationState::Merge { .. }
        ) {
            return Err(AppError::new(
                "merge_not_in_progress",
                "There is no merge to complete.",
            ));
        }
        if !conflicts::list_conflicts(&self.git, path).await?.is_empty() {
            return Err(AppError::new(
                "unresolved_conflicts",
                "Stage every resolved file before completing the merge.",
            ));
        }
        let before_hash = repository::read_head(&self.git, path).await?.0.hash;
        let runner = GitRunner::with_timeout(Duration::from_secs(300));
        let process = runner
            .run_with_input(
                path,
                &["commit", "--cleanup=verbatim", "-F", "-"],
                Some(message.as_bytes()),
            )
            .await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let (git_ms, error) = match process {
            Ok(output) => (
                Some(output.duration.as_millis()),
                (!output.success()).then(|| git_failure(&output)),
            ),
            Err(cause) => (None, Some(AppError::from(cause))),
        };
        let mut result = self
            .refresh_merge(path, &before_hash, error, git_ms)
            .await?;
        result.outcome = if matches!(
            result.state.operation,
            RepositoryOperationState::Merge { .. }
        ) {
            if result.state.conflicts.is_empty() {
                MergeOutcome::NeedsCompletion
            } else {
                MergeOutcome::NeedsResolution
            }
        } else if result.reset_history {
            MergeOutcome::Completed
        } else {
            MergeOutcome::Failed
        };
        Ok(result)
    }

    pub async fn abort_merge(&self) -> Result<MergeMutationResult, AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !matches!(
            conflicts::operation_state(&self.git, path).await?,
            RepositoryOperationState::Merge { .. }
        ) {
            return Err(AppError::new(
                "merge_not_in_progress",
                "There is no merge to abort.",
            ));
        }
        let before_hash = repository::read_head(&self.git, path).await?.0.hash;
        let process = self.git.run(path, &["merge", "--abort"]).await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let (git_ms, error) = match process {
            Ok(output) => (
                Some(output.duration.as_millis()),
                (!output.success()).then(|| git_failure(&output)),
            ),
            Err(cause) => (None, Some(AppError::from(cause))),
        };
        let mut result = self
            .refresh_merge(path, &before_hash, error, git_ms)
            .await?;
        result.outcome = if matches!(result.state.operation, RepositoryOperationState::None) {
            MergeOutcome::Aborted
        } else {
            MergeOutcome::Failed
        };
        Ok(result)
    }

    async fn refresh_merge(
        &self,
        path: &Path,
        before_hash: &str,
        error: Option<AppError>,
        git_duration_ms: Option<u128>,
    ) -> Result<MergeMutationResult, AppError> {
        let started = Instant::now();
        let refresh_error = |cause: AppError| {
            AppError::new(
                "merge_refresh_failed",
                format!(
                    "Git ran, but updated repository state could not be read: {}",
                    cause.message
                ),
            )
        };
        let (branches, _) = branches::load_branches(&self.git, path)
            .await
            .map_err(refresh_error)?;
        let branch = branches
            .branches
            .iter()
            .find(|branch| branch.is_current)
            .map(|branch| branch.name.clone());
        let (head, _) = repository::read_head(&self.git, path)
            .await
            .map_err(refresh_error)?;
        let state = load_repository_state(&self.git, path)
            .await
            .map_err(refresh_error)?;
        let refresh_duration_ms = started.elapsed().as_millis();
        eprintln!("Merge.Refresh git_ms={git_duration_ms:?} refresh_ms={refresh_duration_ms} conflicts={}", state.conflicts.len());
        Ok(MergeMutationResult {
            reset_history: head.hash != before_hash,
            clear_diff: true,
            state,
            branches,
            branch,
            head,
            outcome: MergeOutcome::Failed,
            error,
            git_duration_ms,
            refresh_duration_ms,
        })
    }

    pub async fn rebase_onto(&self, full_ref: &str) -> Result<HistoryMutationResult, AppError> {
        branches::local_name(full_ref)?;
        self.history_mutation(HistoryCommand::Rebase(full_ref))
            .await
    }

    pub async fn cherry_pick(&self, commit: &str) -> Result<HistoryMutationResult, AppError> {
        diff::validate_commit(commit)?;
        self.history_mutation(HistoryCommand::CherryPick(commit))
            .await
    }

    pub async fn revert_commit(&self, commit: &str) -> Result<HistoryMutationResult, AppError> {
        diff::validate_commit(commit)?;
        self.history_mutation(HistoryCommand::Revert(commit)).await
    }

    pub async fn reset_to(
        &self,
        commit: &str,
        mode: ResetMode,
    ) -> Result<HistoryMutationResult, AppError> {
        diff::validate_commit(commit)?;
        self.history_mutation(HistoryCommand::Reset(commit, mode))
            .await
    }

    pub async fn continue_history_operation(&self) -> Result<HistoryMutationResult, AppError> {
        self.history_mutation(HistoryCommand::Continue).await
    }

    pub async fn skip_history_operation(&self) -> Result<HistoryMutationResult, AppError> {
        self.history_mutation(HistoryCommand::Skip).await
    }

    pub async fn abort_history_operation(&self) -> Result<HistoryMutationResult, AppError> {
        self.history_mutation(HistoryCommand::Abort).await
    }

    async fn history_mutation(
        &self,
        action: HistoryCommand<'_>,
    ) -> Result<HistoryMutationResult, AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let before = load_repository_state(&self.git, path).await?;
        let (before_head, _) = repository::read_head(&self.git, path).await?;
        let args: Vec<&str> = match action {
            HistoryCommand::Rebase(full_ref) => {
                require_idle(&before)?;
                let (listed, _) = branches::load_branches(&self.git, path).await?;
                let current_branch = listed
                    .branches
                    .iter()
                    .find(|branch| branch.is_current)
                    .ok_or_else(|| {
                        AppError::new("detached_head", "Check out a local branch before rebasing.")
                    })?;
                if current_branch.full_ref_name == full_ref {
                    return Err(AppError::new(
                        "invalid_rebase_target",
                        "Choose another local branch.",
                    ));
                }
                if !listed.branches.iter().any(|branch| {
                    branch.full_ref_name == full_ref
                        && matches!(branch.kind, branches::BranchKind::Local)
                }) {
                    return Err(AppError::new(
                        "branch_missing",
                        "Choose an existing local branch.",
                    ));
                }
                vec!["rebase", "--", full_ref]
            }
            HistoryCommand::CherryPick(hash) => {
                require_idle(&before)?;
                require_current_branch(&self.git, path).await?;
                reject_merge_commit(&self.git, path, hash).await?;
                vec!["cherry-pick", hash]
            }
            HistoryCommand::Revert(hash) => {
                require_idle(&before)?;
                require_current_branch(&self.git, path).await?;
                reject_merge_commit(&self.git, path, hash).await?;
                vec!["revert", "--no-edit", hash]
            }
            HistoryCommand::Reset(hash, mode) => {
                require_idle(&before)?;
                require_current_branch(&self.git, path).await?;
                if matches!(mode, ResetMode::Hard) {
                    protect_untracked(&self.git, path, hash).await?;
                }
                vec![
                    "reset",
                    match mode {
                        ResetMode::Soft => "--soft",
                        ResetMode::Mixed => "--mixed",
                        ResetMode::Hard => "--hard",
                    },
                    hash,
                ]
            }
            HistoryCommand::Continue => {
                let operation_name = operation_command(&before.operation)?;
                vec!["-c", "core.editor=true", operation_name, "--continue"]
            }
            HistoryCommand::Skip => {
                let operation_name = operation_command(&before.operation)?;
                vec!["-c", "core.editor=true", operation_name, "--skip"]
            }
            HistoryCommand::Abort => {
                let operation_name = operation_command(&before.operation)?;
                vec![operation_name, "--abort"]
            }
        };
        let runner = GitRunner::with_timeout(Duration::from_secs(300));
        let execution = runner.run_limited(path, &args, 1024 * 1024).await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let (git_ms, error) = match execution {
            Ok(output) => (Some(output.duration.as_millis()), (!output.success()).then(|| git_failure(&output))),
            Err(GitRunError::OutputLimit) => (None, Some(AppError::new("operation_output_too_large", "Git output exceeded the safe capture limit. Inspect the refreshed repository state."))),
            Err(cause) => (None, Some(AppError::from(cause))),
        };
        let refreshed = self
            .refresh_merge(path, &before_head.hash, error, git_ms)
            .await?;
        Ok(HistoryMutationResult {
            state: refreshed.state,
            branches: refreshed.branches,
            branch: refreshed.branch,
            head: refreshed.head,
            error: refreshed.error,
            reset_history: refreshed.reset_history,
            clear_diff: refreshed.clear_diff,
            git_duration_ms: refreshed.git_duration_ms,
            refresh_duration_ms: refreshed.refresh_duration_ms,
        })
    }

    pub async fn save_conflict_text(
        &self,
        path: &str,
        expected: Option<&str>,
        text: &str,
    ) -> Result<ConflictMutationResult, AppError> {
        valid_relative_path(path)?;
        let current = self.repository.lock().await;
        let repository = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let files = conflicts::list_conflicts(&self.git, repository).await?;
        conflicts::selected(&files, path)?;
        conflicts::save_text(repository, path, expected, text)?;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.refresh_conflict(repository, None).await
    }

    pub async fn use_conflict_side(
        &self,
        path: &str,
        side: ConflictSide,
    ) -> Result<ConflictMutationResult, AppError> {
        valid_relative_path(path)?;
        let current = self.repository.lock().await;
        let repository = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let files = conflicts::list_conflicts(&self.git, repository).await?;
        let file = conflicts::selected(&files, path)?;
        let chosen = match side {
            ConflictSide::Ours => file.ours.as_ref(),
            ConflictSide::Theirs => file.theirs.as_ref(),
        };
        let execution = if chosen.is_some() {
            let side_arg = match side {
                ConflictSide::Ours => "--ours",
                ConflictSide::Theirs => "--theirs",
            };
            let checkout = self
                .git
                .run(
                    repository,
                    &["--literal-pathspecs", "checkout", side_arg, "--", path],
                )
                .await;
            match checkout {
                Ok(output) if output.success() => self
                    .git
                    .run(
                        repository,
                        &["--literal-pathspecs", "add", "-A", "--", path],
                    )
                    .await
                    .map_err(AppError::from)
                    .and_then(|output| {
                        if output.success() {
                            Ok(())
                        } else {
                            Err(git_failure(&output))
                        }
                    }),
                Ok(output) => Err(git_failure(&output)),
                Err(cause) => Err(AppError::from(cause)),
            }
        } else {
            self.git
                .run(repository, &["--literal-pathspecs", "rm", "-f", "--", path])
                .await
                .map_err(AppError::from)
                .and_then(|output| {
                    if output.success() {
                        Ok(())
                    } else {
                        Err(git_failure(&output))
                    }
                })
        };
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.refresh_conflict(repository, execution.err()).await
    }

    pub async fn stage_conflict(&self, path: &str) -> Result<ConflictMutationResult, AppError> {
        self.mutate_conflict(path, false).await
    }

    pub async fn delete_conflict(&self, path: &str) -> Result<ConflictMutationResult, AppError> {
        self.mutate_conflict(path, true).await
    }

    async fn mutate_conflict(
        &self,
        path: &str,
        delete: bool,
    ) -> Result<ConflictMutationResult, AppError> {
        valid_relative_path(path)?;
        let current = self.repository.lock().await;
        let repository = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let files = conflicts::list_conflicts(&self.git, repository).await?;
        conflicts::selected(&files, path)?;
        let args = if delete {
            vec!["--literal-pathspecs", "rm", "-f", "--", path]
        } else {
            vec!["--literal-pathspecs", "add", "-A", "--", path]
        };
        let execution = self
            .git
            .run(repository, &args)
            .await
            .map_err(AppError::from)
            .and_then(|output| {
                if output.success() {
                    Ok(())
                } else {
                    Err(git_failure(&output))
                }
            });
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.refresh_conflict(repository, execution.err()).await
    }

    async fn refresh_conflict(
        &self,
        repository: &Path,
        error: Option<AppError>,
    ) -> Result<ConflictMutationResult, AppError> {
        let state = load_repository_state(&self.git, repository)
            .await
            .map_err(|cause| {
                AppError::new(
                    "merge_refresh_failed",
                    format!(
                        "Conflict operation ran, but repository state could not be refreshed: {}",
                        cause.message
                    ),
                )
            })?;
        Ok(ConflictMutationResult { state, error })
    }

    pub async fn branches(&self) -> Result<BranchList, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = branches::load_branches(&self.git, &path).await;
        self.ensure_generation(generation)?;
        result.map(|(list, _)| list)
    }

    pub async fn stashes(&self) -> Result<StashList, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = stash::load_stashes(&self.git, &path).await;
        self.ensure_generation(generation)?;
        result.map(|(list, _)| list)
    }

    pub async fn tags(&self) -> Result<TagList, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = tags::load_tags(&self.git, &path).await;
        self.ensure_generation(generation)?;
        result.map(|(list, _)| list)
    }

    pub async fn rename_branch(
        &self,
        full_ref: &str,
        new_name: &str,
    ) -> Result<BranchRefMutationResult, AppError> {
        branches::local_name(full_ref)?;
        branches::validate_name(new_name)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        branches::rename_local(&self.git, path, full_ref, new_name).await?;
        let (branches, _) = branches::load_branches(&self.git, path)
            .await
            .map_err(|error| {
                AppError::new(
                    "branch_refresh_failed",
                    format!(
                        "Branch renamed, but refs could not be refreshed: {}",
                        error.message
                    ),
                )
            })?;
        let branch = branches
            .branches
            .iter()
            .find(|branch| branch.is_current)
            .map(|branch| branch.name.clone());
        Ok(BranchRefMutationResult { branch, branches })
    }

    pub async fn delete_branch(&self, full_ref: &str) -> Result<BranchRefMutationResult, AppError> {
        branches::local_name(full_ref)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        branches::delete_local(&self.git, path, full_ref).await?;
        let (branches, _) = branches::load_branches(&self.git, path)
            .await
            .map_err(|error| {
                AppError::new(
                    "branch_refresh_failed",
                    format!(
                        "Branch deleted, but refs could not be refreshed: {}",
                        error.message
                    ),
                )
            })?;
        let branch = branches
            .branches
            .iter()
            .find(|branch| branch.is_current)
            .map(|branch| branch.name.clone());
        Ok(BranchRefMutationResult { branch, branches })
    }

    pub async fn create_tag(
        &self,
        name: &str,
        annotation: Option<&str>,
        target_hash: Option<&str>,
    ) -> Result<TagMutationResult, AppError> {
        tags::validate_name(name)?;
        let target = if let Some(hash) = target_hash {
            if !((hash.len() == 40 || hash.len() == 64)
                && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            {
                return Err(AppError::new(
                    "invalid_tag_target",
                    "Choose a commit from history.",
                ));
            }
            format!("{hash}^{{commit}}")
        } else {
            "HEAD^{commit}".to_owned()
        };
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        tags::create_tag(&self.git, path, name, &target, annotation).await?;
        let (tags, _) = tags::load_tags(&self.git, path).await.map_err(|error| {
            AppError::new(
                "tag_refresh_failed",
                format!(
                    "Tag created, but tags could not be refreshed: {}",
                    error.message
                ),
            )
        })?;
        Ok(TagMutationResult { tags })
    }

    pub async fn delete_tag(&self, name: &str) -> Result<TagMutationResult, AppError> {
        tags::validate_name(name)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        tags::delete_tag(&self.git, path, name).await?;
        let (tags, _) = tags::load_tags(&self.git, path).await.map_err(|error| {
            AppError::new(
                "tag_refresh_failed",
                format!(
                    "Tag deleted, but tags could not be refreshed: {}",
                    error.message
                ),
            )
        })?;
        Ok(TagMutationResult { tags })
    }

    pub async fn create_stash(
        &self,
        message: Option<&str>,
    ) -> Result<StashMutationResult, AppError> {
        if let Some(message) = message {
            stash::validate_message(message)?;
        }
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        require_no_operation(&self.git, path).await?;
        let (before, _) = load_status(&self.git, path).await?;
        if before.staged.is_empty() && before.unstaged.is_empty() && before.conflicted.is_empty() {
            return Err(AppError::new(
                "no_changes_to_stash",
                "There are no tracked or staged changes to stash.",
            ));
        }
        let execution = stash::create(&self.git, path, message).await;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.finish_stash_mutation(path, execution, true, true, true)
            .await
    }

    pub async fn apply_stash(&self, hash: &str) -> Result<StashMutationResult, AppError> {
        self.use_stash(hash, StashAction::Apply).await
    }

    pub async fn pop_stash(&self, hash: &str) -> Result<StashMutationResult, AppError> {
        self.use_stash(hash, StashAction::Pop).await
    }

    pub async fn drop_stash(&self, hash: &str) -> Result<StashMutationResult, AppError> {
        self.use_stash(hash, StashAction::Drop).await
    }

    async fn use_stash(
        &self,
        hash: &str,
        action: StashAction,
    ) -> Result<StashMutationResult, AppError> {
        stash::validate_hash(hash)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !matches!(action, StashAction::Drop) {
            require_no_operation(&self.git, path).await?;
        }
        let (before, _) = stash::load_stashes(&self.git, path).await?;
        let entry = stash::resolve(&before, hash)?;
        let execution = match action {
            StashAction::Apply => stash::apply(&self.git, path, hash).await,
            StashAction::Pop => stash::pop(&self.git, path, &entry.reference).await,
            StashAction::Drop => stash::drop_stash(&self.git, path, &entry.reference).await,
        };
        let changes_worktree = !matches!(action, StashAction::Drop);
        if changes_worktree {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        self.finish_stash_mutation(
            path,
            execution,
            changes_worktree,
            changes_worktree,
            !matches!(action, StashAction::Apply),
        )
        .await
    }

    async fn finish_stash_mutation(
        &self,
        path: &Path,
        execution: Result<crate::git::GitOutput, AppError>,
        refresh_status: bool,
        clear_diff: bool,
        reset_history: bool,
    ) -> Result<StashMutationResult, AppError> {
        // A failed apply/pop can leave an altered index or conflicted files.
        // Refresh after process errors as well as non-zero Git exit codes.
        let error = match execution {
            Ok(output) if output.success() => None,
            Ok(output) => Some(git_failure(&output)),
            Err(error) => Some(error),
        };
        let (status, operation, conflicts) = if refresh_status {
            let state = load_repository_state(&self.git, path).await.map_err(|cause| {
                AppError::new("stash_refresh_failed", format!("Stash operation finished, but repository state could not be refreshed: {}", cause.message))
            })?;
            (
                Some(state.status),
                Some(state.operation),
                Some(state.conflicts),
            )
        } else {
            (None, None, None)
        };
        let stashes = stash::load_stashes(&self.git, path)
            .await
            .map_err(|cause| {
                AppError::new(
                    "stash_refresh_failed",
                    format!(
                        "Stash operation finished, but the stash list could not be refreshed: {}",
                        cause.message
                    ),
                )
            })?
            .0;
        Ok(StashMutationResult {
            stashes,
            status,
            operation,
            conflicts,
            reset_history,
            clear_diff,
            error,
        })
    }

    /// Creating a ref leaves HEAD, index, working tree, and history topology alone.
    pub async fn create_branch(&self, name: &str) -> Result<BranchList, AppError> {
        branches::validate_name(name)?;
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        let output = self
            .git
            .run(path, &["branch", "--", name])
            .await
            .map_err(AppError::from)?;
        if !output.success() {
            return Err(git_failure(&output));
        }
        branches::load_branches(&self.git, path)
            .await
            .map(|(list, _)| list)
            .map_err(|error| {
                AppError::new(
                    "branch_refresh_failed",
                    format!(
                        "Branch created, but the list could not be refreshed: {}",
                        error.message
                    ),
                )
            })
    }

    /// A successful switch invalidates in-flight history and diff requests.
    /// Only HEAD, status, and refs are refreshed; the UI resets history and diff.
    pub async fn checkout_branch(&self, name: &str) -> Result<CheckoutResult, AppError> {
        branches::validate_name(name)?;
        let started = Instant::now();
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        require_no_operation(&self.git, path).await?;
        let output = self
            .git
            .run(path, &["switch", "--no-guess", "--", name])
            .await
            .map_err(AppError::from)?;
        if !output.success() {
            return Err(git_failure(&output));
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        let refresh_error = |error: AppError| {
            AppError::new(
                "checkout_refresh_failed",
                format!(
                    "Branch switched, but repository state could not be refreshed: {}",
                    error.message
                ),
            )
        };
        let (branches, branch_timing) = branches::load_branches(&self.git, path)
            .await
            .map_err(refresh_error)?;
        let branch = branches
            .branches
            .iter()
            .find(|branch| branch.is_current)
            .map(|branch| branch.name.clone());
        let (head, head_duration) = repository::read_head(&self.git, path)
            .await
            .map_err(refresh_error)?;
        let (status, status_timing) = load_status(&self.git, path).await.map_err(refresh_error)?;
        eprintln!("Branch.Checkout switch_ms={} branches_ms={} head_ms={} status_ms={} total_ms={} processes=4", output.duration.as_millis(), branch_timing.total.as_millis(), head_duration.as_millis(), status_timing.total.as_millis(), started.elapsed().as_millis());
        Ok(CheckoutResult {
            branch,
            head,
            status,
            branches,
        })
    }

    async fn mutate(&self, args: &[&str]) -> Result<RepositoryStatus, AppError> {
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        if !path.is_dir() {
            return Err(AppError::new(
                "repository_missing",
                "The open repository directory no longer exists.",
            ));
        }
        let output = self.git.run(path, args).await.map_err(AppError::from)?;
        if !output.success() {
            return Err(git_failure(&output));
        }
        let (status, _) = load_status(&self.git, path).await.map_err(|error| {
            AppError::new(
                "status_refresh_failed",
                format!(
                    "The Git operation succeeded, but status could not be refreshed: {}",
                    error.message
                ),
            )
        })?;
        Ok(status)
    }

    pub async fn stage_file(
        &self,
        path: &str,
        old_path: Option<&str>,
    ) -> Result<RepositoryStatus, AppError> {
        valid_relative_path(path)?;
        if let Some(old) = old_path {
            valid_relative_path(old)?;
        }
        let mut args = vec!["--literal-pathspecs", "add", "-A", "--", path];
        if let Some(old) = old_path {
            args.push(old);
        }
        self.mutate(&args).await
    }

    pub async fn unstage_file(
        &self,
        path: &str,
        old_path: Option<&str>,
    ) -> Result<RepositoryStatus, AppError> {
        valid_relative_path(path)?;
        if let Some(old) = old_path {
            valid_relative_path(old)?;
        }
        let mut args = vec!["--literal-pathspecs", "reset", "-q", "--", path];
        if let Some(old) = old_path {
            args.push(old);
        }
        self.mutate(&args).await
    }

    pub async fn stage_all(&self) -> Result<RepositoryStatus, AppError> {
        self.mutate(&["add", "-A"]).await
    }

    pub async fn unstage_all(&self) -> Result<RepositoryStatus, AppError> {
        self.mutate(&["reset", "-q", "--", "."]).await
    }

    pub async fn partial_stage(
        &self,
        path: &str,
        staged: bool,
        selection: PartialSelection,
    ) -> Result<PartialStageResult, AppError> {
        valid_relative_path(path)?;
        let current = self.repository.lock().await;
        let repository = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        let state = load_repository_state(&self.git, repository).await?;
        require_idle(&state)?;
        let group = if staged {
            &state.status.staged
        } else {
            &state.status.unstaged
        };
        let change = group
            .iter()
            .find(|change| change.path == path)
            .ok_or_else(|| {
                AppError::new(
                    "stale_diff",
                    "This change is no longer in the selected section. Refresh and try again.",
                )
            })?;
        if change.kind != crate::status::ChangeKind::Modified || change.old_path.is_some() {
            return Err(AppError::new("partial_unsupported", "Partial staging is available for modified tracked text files. Use the whole-file action for this change."));
        }
        let raw = diff::load_working_patch(&self.git, repository, staged, false, change).await?;
        let parsed = diff::parse_patch(&raw.stdout, change.clone())?;
        if !parsed.partial_stageable {
            return Err(AppError::new(
                "partial_unsupported",
                "This file has binary, mode, or other changes that require the whole-file action.",
            ));
        }
        let patch = match partial_stage::build_patch(&raw.stdout, &parsed, &selection, staged) {
            Ok(patch) => patch,
            Err(error) if error.code == "stale_diff" => {
                return refresh_partial(&self.git, repository, path, Some(error)).await
            }
            Err(error) => return Err(error),
        };
        let args = if staged {
            &[
                "apply",
                "--cached",
                "--reverse",
                "--recount",
                "--whitespace=nowarn",
                "-",
            ][..]
        } else {
            &["apply", "--cached", "--recount", "--whitespace=nowarn", "-"][..]
        };
        let started = Instant::now();
        let output = match self
            .git
            .run_with_input(repository, args, Some(&patch))
            .await
        {
            Ok(output) => output,
            Err(cause) => {
                return refresh_partial(&self.git, repository, path, Some(AppError::from(cause)))
                    .await
            }
        };
        let error = (!output.success()).then(|| git_failure(&output));
        let result = refresh_partial(&self.git, repository, path, error).await?;
        eprintln!(
            "Git.PartialStage git_ms={} refresh_ms={} patch_bytes={}",
            output.duration.as_millis(),
            started
                .elapsed()
                .saturating_sub(output.duration)
                .as_millis(),
            patch.len()
        );
        Ok(result)
    }

    pub async fn commit(&self, message: &str) -> Result<CommitResult, AppError> {
        if message.trim().is_empty() {
            return Err(AppError::new(
                "empty_commit_message",
                "Enter a commit message.",
            ));
        }
        if message.as_bytes().contains(&0) || message.len() > 1024 * 1024 {
            return Err(AppError::new(
                "invalid_commit_message",
                "The commit message contains unsupported content or is too large.",
            ));
        }
        let started = Instant::now();
        let current = self.repository.lock().await;
        let path = current
            .as_deref()
            .ok_or_else(|| AppError::new("no_repository", "Open a repository first."))?;
        require_no_operation(&self.git, path).await?;
        let (before, preflight_timing) = load_status(&self.git, path).await?;
        if !before.conflicted.is_empty() {
            return Err(AppError::new(
                "unresolved_conflicts",
                "Resolve the conflicted files before committing.",
            ));
        }
        if before.staged.is_empty() {
            return Err(AppError::new(
                "no_staged_changes",
                "Stage at least one change before committing.",
            ));
        }

        // A longer timeout allows normal hooks and signing to finish. The message
        // goes through stdin, never through a shell or command-line escaping.
        let commit_runner = GitRunner::with_timeout(Duration::from_secs(300));
        let output = commit_runner
            .run_with_input(
                path,
                &["commit", "--cleanup=verbatim", "-F", "-"],
                Some(message.as_bytes()),
            )
            .await
            .map_err(AppError::from)?;
        if !output.success() {
            return Err(git_failure(&output));
        }

        self.generation.fetch_add(1, Ordering::SeqCst);

        let (head, head_duration) =
            repository::read_head(&self.git, path)
                .await
                .map_err(|error| {
                    AppError::new(
                        "commit_refresh_failed",
                        format!(
                            "Commit succeeded, but HEAD could not be refreshed: {}",
                            error.message
                        ),
                    )
                })?;
        let (status, status_timing) = load_status(&self.git, path).await.map_err(|error| {
            AppError::new(
                "commit_refresh_failed",
                format!(
                    "Commit succeeded, but status could not be refreshed: {}",
                    error.message
                ),
            )
        })?;
        eprintln!(
            "Commit.Workflow preflight_ms={} commit_ms={} head_ms={} status_ms={} total_ms={} processes=4",
            preflight_timing.total.as_millis(), output.duration.as_millis(), head_duration.as_millis(),
            status_timing.total.as_millis(), started.elapsed().as_millis()
        );
        Ok(CommitResult { head, status })
    }
}
