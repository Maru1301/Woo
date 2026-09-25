use crate::{
    branches::{self, BranchList},
    diff::{self, DiffFile},
    error::{git_failure, AppError},
    git::GitRunner,
    history::{self, CommitHistoryPage},
    remotes::{self, RemoteList},
    repository::{self, HeadInfo, RepositoryInfo},
    status::{parse_status, FileChange, RepositoryStatus},
};
use serde::Serialize;
use std::{
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{watch, Mutex};

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
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRefresh {
    pub branches: BranchList,
    pub head: Option<HeadInfo>,
    pub status: Option<RepositoryStatus>,
    pub reset_history: bool,
    pub refresh_history: bool,
    pub clear_diff: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteOperationStatus {
    pub id: u64,
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
pub struct CheckoutResult {
    pub branch: Option<String>,
    pub head: HeadInfo,
    pub status: RepositoryStatus,
    pub branches: BranchList,
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

/// M2 has one open repository. The mutex serializes repository switches, status
/// reads, and mutation-plus-refresh so callers receive a consistent snapshot.
pub struct WorkingTree {
    git: GitRunner,
    repository: Mutex<Option<PathBuf>>,
    generation: AtomicU64,
    remote_task: Mutex<Option<RemoteTask>>,
    next_operation_id: AtomicU64,
}

impl Default for WorkingTree {
    fn default() -> Self {
        Self {
            git: GitRunner::with_timeout(Duration::from_secs(60)),
            repository: Mutex::new(None),
            generation: AtomicU64::new(0),
            remote_task: Mutex::new(None),
            next_operation_id: AtomicU64::new(0),
        }
    }
}

impl WorkingTree {
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
        *current = None;
        let info = repository::open(&self.git, path).await?;
        *current = Some(PathBuf::from(&info.path));
        Ok(info)
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

    pub async fn start_remote(
        self: &Arc<Self>,
        kind: RemoteKind,
        remote: Option<&str>,
    ) -> Result<RemoteOperationStatus, AppError> {
        let (path, generation) = self.snapshot().await?;
        let remote = if matches!(kind, RemoteKind::Fetch) {
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
        let id = self.next_operation_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (cancel, receiver) = watch::channel(false);
        let status = RemoteOperationStatus {
            id,
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

    async fn update_remote(&self, id: u64, update: impl FnOnce(&mut RemoteOperationStatus)) {
        let mut slot = self.remote_task.lock().await;
        if let Some(task) = slot.as_mut().filter(|task| task.status.id == id) {
            update(&mut task.status);
            task.status.elapsed_ms = task.started.elapsed().as_millis();
        }
    }

    async fn run_remote_task(
        self: Arc<Self>,
        id: u64,
        generation: u64,
        kind: RemoteKind,
        remote: Option<String>,
        cancelled: watch::Receiver<bool>,
    ) {
        let outcome = self
            .execute_remote(id, generation, kind, remote.as_deref(), cancelled)
            .await;
        self.update_remote(id, |status| match outcome {
            Ok(completion) => {
                status.phase = match completion.error.as_ref().map(|error| error.code) {
                    Some("operation_cancelled") => RemotePhase::Cancelled,
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
                } else {
                    RemotePhase::Failed
                };
                status.error = Some(error);
            }
        })
        .await;
    }

    async fn execute_remote(
        &self,
        id: u64,
        generation: u64,
        kind: RemoteKind,
        remote: Option<&str>,
        cancelled: watch::Receiver<bool>,
    ) -> Result<RemoteCompletion, AppError> {
        let current = self.repository.lock().await;
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
        self.update_remote(id, |status| status.phase = RemotePhase::Running)
            .await;
        let args: Vec<&str> = match kind {
            RemoteKind::Fetch => vec!["fetch", "--", remote.expect("validated fetch remote")],
            RemoteKind::Pull => vec!["pull"],
            RemoteKind::Push => vec!["push"],
        };
        let process = self.git.run_remote(path, &args, cancelled, None).await;
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
        let (head, status) = if matches!(kind, RemoteKind::Pull) {
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
            (Some(head), Some(status))
        } else {
            (None, None)
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

    pub async fn branches(&self) -> Result<BranchList, AppError> {
        let (path, generation) = self.snapshot().await?;
        let result = branches::load_branches(&self.git, &path).await;
        self.ensure_generation(generation)?;
        result.map(|(list, _)| list)
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
