//! Filesystem notifications are hints. Every emitted update is validated through
//! the existing Git-owned repository operations while holding the session lock.
use crate::{
    branches::BranchList,
    conflicts::RepositoryState,
    error::AppError,
    git::GitRunner,
    repository::{HeadInfo, RepositoryInfo},
    working_tree::WorkingTree,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
    time,
};

const QUIET_PERIOD: Duration = Duration::from_millis(250);
const MAX_BURST: Duration = Duration::from_secs(1);
const MAX_PATHS: usize = 64;

#[derive(Debug)]
pub struct ValidatedState {
    pub identity: Option<(Option<String>, Option<HeadInfo>)>,
    pub state: Option<RepositoryState>,
    pub branches: Option<BranchList>,
}

#[derive(Clone, Default, Debug)]
struct Hints {
    worktree: bool,
    index: bool,
    head: bool,
    refs: bool,
    tags: bool,
    operation: bool,
    unknown: bool,
    paths: BTreeSet<String>,
    events: u32,
}

impl Hints {
    fn absorb(&mut self, other: Self) {
        self.worktree |= other.worktree;
        self.index |= other.index;
        self.head |= other.head;
        self.refs |= other.refs;
        self.tags |= other.tags;
        self.operation |= other.operation;
        self.unknown |= other.unknown;
        self.events = self.events.saturating_add(other.events);
        for path in other.paths {
            if self.paths.len() < MAX_PATHS {
                self.paths.insert(path);
            } else {
                self.unknown = true;
            }
        }
    }

    fn relevant(&self) -> bool {
        self.worktree || self.index || self.head || self.refs || self.operation || self.unknown
    }
}

async fn next_burst(rx: &mut mpsc::Receiver<Hints>) -> Option<Hints> {
    let mut hints = rx.recv().await?;
    let burst_start = Instant::now();
    while burst_start.elapsed() < MAX_BURST {
        match time::timeout(QUIET_PERIOD, rx.recv()).await {
            Ok(Some(next)) => hints.absorb(next),
            _ => break,
        }
    }
    Some(hints)
}

#[derive(Clone)]
struct Locations {
    root: PathBuf,
    git_dirs: Vec<PathBuf>,
}

impl Locations {
    fn classify(&self, path: &Path) -> Hints {
        let mut hint = Hints {
            events: 1,
            ..Hints::default()
        };
        if path == self.root {
            hint.unknown = true;
            return hint;
        }
        for dir in &self.git_dirs {
            if let Ok(relative) = path.strip_prefix(dir) {
                if relative.as_os_str().is_empty() {
                    hint.unknown = true;
                    return hint;
                }
                let name = relative.to_string_lossy().replace('\\', "/");
                let name = name.as_str();
                if name == "HEAD" || name == "ORIG_HEAD" || name == "HEAD.lock" {
                    hint.head = true;
                } else if name == "index" || name == "index.lock" {
                    hint.index = true;
                } else if name.starts_with("refs/")
                    || name == "packed-refs"
                    || name == "packed-refs.lock"
                {
                    hint.refs = true;
                    hint.tags = name.starts_with("refs/tags/") || name.starts_with("packed-refs");
                } else if name == "MERGE_HEAD"
                    || name == "MERGE_MSG"
                    || name == "CHERRY_PICK_HEAD"
                    || name == "REVERT_HEAD"
                    || name.starts_with("rebase-merge")
                    || name.starts_with("rebase-apply")
                    || name == "sequencer"
                    || name.starts_with("sequencer/")
                {
                    hint.operation = true;
                } else if name == "config" || name == "commondir" {
                    hint.unknown = true;
                }
                // Object writes, reflogs, temporary files and lock churn do not
                // independently change a view. Their corresponding refs/index do.
                return hint;
            }
        }
        if let Ok(relative) = path.strip_prefix(&self.root) {
            if relative == Path::new(".git") {
                hint.unknown = true;
            } else {
                hint.worktree = true;
                if let Some(value) = relative.to_str() {
                    hint.paths.insert(value.replace('\\', "/"));
                } else {
                    hint.unknown = true;
                }
            }
        }
        hint
    }
}

async fn locations(git: &GitRunner, root: &Path) -> Result<Locations, AppError> {
    let output = git
        .run(
            root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(AppError::new(
            "watch_setup",
            "Could not locate Git metadata for auto refresh.",
        ));
    }
    let mut git_dirs = Vec::new();
    for line in output
        .stdout_text()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let dir = PathBuf::from(line);
        if !dir.is_dir() {
            return Err(AppError::new("watch_setup", "Git metadata is unavailable."));
        }
        if !git_dirs.contains(&dir) {
            git_dirs.push(dir);
        }
    }
    if git_dirs.is_empty() {
        return Err(AppError::new("watch_setup", "Git metadata is unavailable."));
    }
    Ok(Locations {
        root: root.to_owned(),
        git_dirs,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRefreshEvent {
    pub repository_id: Option<String>,
    pub session_id: u64,
    pub sequence: u64,
    pub state: Option<RepositoryState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<Option<HeadInfo>>,
    pub branches: Option<BranchList>,
    pub reset_history: bool,
    pub refresh_history: bool,
    pub refresh_tags: bool,
    pub clear_diff: bool,
    pub diff_paths: Vec<String>,
    pub unavailable: Option<AppError>,
    pub coalesced_events: u32,
    pub validation_ms: u128,
    pub git_process_count: u8,
}

struct ActiveWatch {
    _watcher: RecommendedWatcher,
    task: JoinHandle<()>,
    request: mpsc::Sender<Hints>,
    repository_id: Option<String>,
}

#[derive(Default)]
pub struct RepositoryWatchManager {
    active: Mutex<Option<ActiveWatch>>,
}

impl RepositoryWatchManager {
    pub async fn stop(&self) {
        if let Some(active) = self.active.lock().await.take() {
            active.task.abort();
        }
    }

    pub async fn revalidate(&self, repository_id: &str) {
        let request = self
            .active
            .lock()
            .await
            .as_ref()
            .filter(|active| active.repository_id.as_deref() == Some(repository_id))
            .map(|active| active.request.clone());
        if let Some(request) = request {
            let _ = request
                .send(Hints {
                    unknown: true,
                    events: 1,
                    ..Hints::default()
                })
                .await;
        }
    }

    pub async fn start(
        &self,
        app: &AppHandle,
        tree: Arc<WorkingTree>,
        info: &RepositoryInfo,
    ) -> Result<(), AppError> {
        self.stop().await;
        let root = PathBuf::from(&info.path);
        let places = locations(&GitRunner::with_timeout(Duration::from_secs(60)), &root).await?;
        let (tx, mut rx) = mpsc::channel::<Hints>(1024);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_places = places.clone();
        let request = tx.clone();
        let callback_overflow = Arc::clone(&overflow);
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let mut hints = Hints::default();
                match result {
                    Ok(event) => {
                        if !matches!(event.kind, notify::EventKind::Access(_)) {
                            if event.paths.is_empty() {
                                hints.unknown = true;
                            }
                            for path in &event.paths {
                                hints.absorb(callback_places.classify(path));
                            }
                        }
                    }
                    Err(_) => hints.unknown = true,
                }
                if hints.relevant() && tx.try_send(hints).is_err() {
                    callback_overflow.store(true, Ordering::SeqCst);
                }
            })
            .map_err(|_| {
                AppError::new("watch_setup", "Could not start repository auto refresh.")
            })?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|_| AppError::new("watch_setup", "Could not watch the repository."))?;
        if let Some(parent) = root.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .map_err(|_| {
                    AppError::new("watch_setup", "Could not watch repository availability.")
                })?;
        }
        for dir in &places.git_dirs {
            if !dir.starts_with(&root) {
                watcher
                    .watch(dir, RecursiveMode::Recursive)
                    .map_err(|_| AppError::new("watch_setup", "Could not watch Git metadata."))?;
            }
        }
        let app = app.clone();
        let session_id = info.session_id;
        let repository_id = tree.repository_id();
        let event_repository_id = repository_id.clone();
        let mut last_identity = (info.branch.clone(), info.head.clone());
        let task = tokio::spawn(async move {
            let mut sequence = 0;
            let mut unavailable = false;
            while let Some(mut hints) = next_burst(&mut rx).await {
                if overflow.swap(false, Ordering::SeqCst) {
                    hints.unknown = true;
                }
                let started = Instant::now();
                let read_identity = hints.head || hints.refs || hints.unknown;
                let read_state =
                    hints.worktree || hints.index || hints.head || hints.operation || hints.unknown;
                let read_branches = hints.refs || hints.head || hints.unknown;
                let result = tree
                    .watch_snapshot(
                        session_id,
                        read_state,
                        read_identity,
                        read_branches,
                        last_identity.1.as_ref().map(|head| head.hash.as_str()),
                    )
                    .await;
                sequence += 1;
                match result {
                    Ok(snapshot) => {
                        unavailable = false;
                        let changed_head = snapshot
                            .identity
                            .as_ref()
                            .is_some_and(|identity| *identity != last_identity);
                        if let Some(identity) = &snapshot.identity {
                            last_identity = identity.clone();
                        }
                        let event = AutoRefreshEvent {
                            repository_id: event_repository_id.clone(),
                            session_id,
                            sequence,
                            state: snapshot.state,
                            branch: snapshot
                                .identity
                                .as_ref()
                                .map(|identity| identity.0.clone()),
                            head: snapshot.identity.map(|identity| identity.1),
                            branches: snapshot.branches,
                            reset_history: changed_head,
                            refresh_history: hints.refs && !changed_head,
                            refresh_tags: hints.tags || hints.unknown,
                            clear_diff: changed_head
                                || hints.index
                                || hints.operation
                                || hints.unknown,
                            diff_paths: hints.paths.into_iter().collect(),
                            unavailable: None,
                            coalesced_events: hints.events,
                            validation_ms: started.elapsed().as_millis(),
                            git_process_count: (if read_identity { 2 } else { 0 })
                                + (if read_state || changed_head { 2 } else { 0 })
                                + (if read_branches { 1 } else { 0 }),
                        };
                        let _ = app.emit("repository-auto-refresh", &event);
                    }
                    Err(error) if error.code == "repository_changed" => break,
                    Err(error) => {
                        if !unavailable {
                            unavailable = true;
                            let event = AutoRefreshEvent {
                                repository_id: event_repository_id.clone(),
                                session_id,
                                sequence,
                                state: None,
                                branch: None,
                                head: None,
                                branches: None,
                                reset_history: false,
                                refresh_history: false,
                                refresh_tags: false,
                                clear_diff: true,
                                diff_paths: Vec::new(),
                                unavailable: Some(error),
                                coalesced_events: hints.events,
                                validation_ms: started.elapsed().as_millis(),
                                git_process_count: 0,
                            };
                            let _ = app.emit("repository-auto-refresh", &event);
                        }
                    }
                }
            }
        });
        *self.active.lock().await = Some(ActiveWatch {
            _watcher: watcher,
            task,
            request,
            repository_id,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_metadata_and_worktree() {
        let places = Locations {
            root: PathBuf::from("/repo"),
            git_dirs: vec![PathBuf::from("/repo/.git"), PathBuf::from("/shared/git")],
        };
        assert!(places.classify(Path::new("/repo/src/a.rs")).worktree);
        assert!(places.classify(Path::new("/repo/.git/index")).index);
        assert!(places.classify(Path::new("/repo/.git/HEAD")).head);
        assert!(places.classify(Path::new("/shared/git/refs/tags/v1")).tags);
        assert!(
            places
                .classify(Path::new("/repo/.git/rebase-merge/head-name"))
                .operation
        );
        assert!(
            places
                .classify(Path::new("/repo/.git/rebase-apply/next"))
                .operation
        );
        assert!(
            places
                .classify(Path::new("/repo/.git/CHERRY_PICK_HEAD"))
                .operation
        );
        assert!(
            places
                .classify(Path::new("/repo/.git/REVERT_HEAD"))
                .operation
        );
        assert!(!places
            .classify(Path::new("/repo/.git/objects/a"))
            .relevant());
    }
    #[test]
    fn coalesces_events_and_bounds_paths() {
        let mut hints = Hints::default();
        for index in 0..100 {
            hints.absorb(Hints {
                worktree: true,
                paths: [format!("{index}.txt")].into(),
                events: 1,
                ..Hints::default()
            });
        }
        assert_eq!(hints.events, 100);
        assert_eq!(hints.paths.len(), MAX_PATHS);
        assert!(hints.unknown);
    }

    #[tokio::test]
    async fn rapid_hints_trigger_one_validation_cycle() {
        let (tx, mut rx) = mpsc::channel(128);
        for _ in 0..100 {
            tx.try_send(Hints {
                index: true,
                events: 1,
                ..Hints::default()
            })
            .unwrap();
        }
        let started = Instant::now();
        let burst = next_burst(&mut rx).await.unwrap();
        assert_eq!(burst.events, 100);
        assert!(burst.index);
        assert!(started.elapsed() >= QUIET_PERIOD);
        assert!(rx.try_recv().is_err());
    }
    #[tokio::test]
    async fn native_worktree_event_is_observed() {
        let directory = tempfile::TempDir::new().unwrap();
        let root = directory.path().to_owned();
        let places = Locations {
            root: root.clone(),
            git_dirs: Vec::new(),
        };
        let (tx, mut rx) = mpsc::channel(16);
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                if let Ok(event) = result {
                    for path in event.paths {
                        let hint = places.classify(&path);
                        if hint.worktree {
                            let _ = tx.try_send(hint);
                        }
                    }
                }
            })
            .unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        std::fs::write(root.join("outside.txt"), "external edit").unwrap();
        let observed = time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(observed.paths.contains("outside.txt"));
    }
    #[test]
    fn absent_identity_is_distinct_from_unborn_head() {
        let event = AutoRefreshEvent {
            repository_id: Some("r1".into()),
            session_id: 1,
            sequence: 1,
            state: None,
            branch: None,
            head: None,
            branches: None,
            reset_history: false,
            refresh_history: false,
            refresh_tags: false,
            clear_diff: false,
            diff_paths: Vec::new(),
            unavailable: None,
            coalesced_events: 1,
            validation_ms: 0,
            git_process_count: 0,
        };
        let absent = serde_json::to_value(&event).unwrap();
        assert_eq!(absent["repositoryId"], "r1");
        assert!(absent.get("head").is_none());
        let unborn = serde_json::to_value(AutoRefreshEvent {
            head: Some(None),
            branch: Some(Some("main".into())),
            ..event
        })
        .unwrap();
        assert!(unborn.get("head").unwrap().is_null());
    }
}
