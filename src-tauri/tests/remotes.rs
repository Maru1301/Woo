use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use woo_lib::{
    git::{GitRunError, GitRunner},
    operation_log::{OperationPhase, OperationSource},
    working_tree::{RemoteKind, RemoteOperationStatus, RemotePhase, WorkingTree},
};

fn output(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}
fn git(directory: &Path, args: &[&str]) -> String {
    let result = output(directory, args);
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}
struct Fixture {
    root: TempDir,
    a: PathBuf,
    b: PathBuf,
    bare: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let bare = root.path().join("remote.git");
        let a = root.path().join("a");
        let b = root.path().join("b");
        fs::create_dir(&bare).unwrap();
        fs::create_dir(&a).unwrap();
        git(&bare, &["init", "--bare"]);
        git(&a, &["init", "-b", "main"]);
        Self::identity(&a);
        fs::write(a.join("file.txt"), "initial\n").unwrap();
        git(&a, &["add", "file.txt"]);
        git(&a, &["commit", "-m", "Initial"]);
        git(&a, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&a, &["push", "-u", "origin", "main"]);
        git(
            root.path(),
            &[
                "clone",
                "--branch",
                "main",
                bare.to_str().unwrap(),
                b.to_str().unwrap(),
            ],
        );
        Self::identity(&b);
        Self { root, a, b, bare }
    }
    fn identity(path: &Path) {
        git(path, &["config", "user.name", "Test User"]);
        git(path, &["config", "user.email", "test@example.com"]);
    }
    fn commit(path: &Path, content: &str, message: &str) -> String {
        fs::write(path.join("file.txt"), content).unwrap();
        git(path, &["add", "file.txt"]);
        git(path, &["commit", "-m", message]);
        git(path, &["rev-parse", "HEAD"])
    }
}
async fn wait(tree: &WorkingTree, id: u64) -> RemoteOperationStatus {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = tree.remote_status(id).await.unwrap();
            if !matches!(status.phase, RemotePhase::Queued | RemotePhase::Running) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn fetch_updates_remote_refs_without_touching_worktree() {
    let f = Fixture::new();
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    let list = tree.remotes().await.unwrap();
    assert_eq!(list.remotes.len(), 1);
    assert_eq!(list.remotes[0].name, "origin");
    let new_head = Fixture::commit(&f.a, "from a\n", "From A");
    git(&f.a, &["push"]);
    let start = Instant::now();
    let started = tree
        .start_remote(RemoteKind::Fetch, Some("origin"))
        .await
        .unwrap();
    assert_eq!(
        tree.start_remote(RemoteKind::Push, None)
            .await
            .unwrap_err()
            .code,
        "operation_busy"
    );
    let finished = wait(&tree, started.id).await;
    let history = tree.operation_history().await;
    assert_eq!(history[0].source, OperationSource::User);
    assert_eq!(history[0].phase, OperationPhase::Completed);
    assert!(
        matches!(finished.phase, RemotePhase::Completed),
        "{:?}",
        finished.error
    );
    assert_eq!(
        git(&f.b, &["rev-parse", "refs/remotes/origin/main"]),
        new_head
    );
    assert_eq!(
        fs::read_to_string(f.b.join("file.txt")).unwrap().trim_end(),
        "initial"
    );
    assert!(finished.refresh.as_ref().unwrap().status.is_none());
    eprintln!(
        "Remote.Fetch total_ms={} git_ms={:?} refresh_ms={:?}",
        start.elapsed().as_millis(),
        finished.git_duration_ms,
        finished.refresh_duration_ms
    );
}

#[tokio::test]
async fn background_fetch_uses_active_session_and_logs_one_semantic_operation() {
    let fixture = Fixture::new();
    let tree = Arc::new(WorkingTree::default());
    let info = tree.open(fixture.b.to_str().unwrap()).await.unwrap();
    assert!(tree
        .start_background_fetch(info.session_id + 1)
        .await
        .unwrap()
        .is_none());
    let new_head = Fixture::commit(&fixture.a, "background update\n", "Background update");
    git(&fixture.a, &["push"]);
    let mut completion = tree.remote_completions();
    let operation_started = Instant::now();
    let started = tree
        .start_background_fetch(info.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(started.source, OperationSource::Background);
    let done = tokio::time::timeout(Duration::from_secs(20), completion.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.id, started.id);
    assert!(
        matches!(done.phase, RemotePhase::Completed),
        "{:?}",
        done.error
    );
    assert!(done.refresh.unwrap().status.is_none());
    eprintln!(
        "F3 local background fetch total_ms={} git_ms={:?} refresh_ms={:?}",
        operation_started.elapsed().as_millis(),
        done.git_duration_ms,
        done.refresh_duration_ms
    );
    assert_eq!(
        git(&fixture.b, &["rev-parse", "refs/remotes/origin/main"]),
        new_head
    );
    let history = tree.operation_history().await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].source, OperationSource::Background);
    assert_eq!(history[0].phase, OperationPhase::Completed);
    assert!(tree
        .start_background_fetch(info.session_id)
        .await
        .unwrap()
        .is_some());
    tree.close().await;
    assert!(tree
        .start_background_fetch(info.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn background_fetch_skips_an_active_merge_without_logging_a_fetch() {
    let fixture = Fixture::new();
    git(&fixture.b, &["checkout", "-b", "feature"]);
    Fixture::commit(&fixture.b, "feature\n", "Feature");
    git(&fixture.b, &["checkout", "main"]);
    Fixture::commit(&fixture.b, "main\n", "Main");
    assert!(!output(&fixture.b, &["merge", "feature"]).status.success());
    let tree = Arc::new(WorkingTree::default());
    let opened = tree.open(fixture.b.to_str().unwrap()).await.unwrap();
    assert!(tree
        .start_background_fetch(opened.session_id)
        .await
        .unwrap()
        .is_none());
    assert!(tree.operation_history().await.is_empty());
}

#[tokio::test]
async fn repository_without_remotes_lists_empty() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    assert!(tree.remotes().await.unwrap().remotes.is_empty());
}

#[tokio::test]
async fn pull_updates_head_status_and_history() {
    let f = Fixture::new();
    let new_head = Fixture::commit(&f.a, "pulled\n", "Pulled");
    git(&f.a, &["push"]);
    git(&f.b, &["config", "pull.ff", "only"]);
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    let started = tree.start_remote(RemoteKind::Pull, None).await.unwrap();
    let finished = wait(&tree, started.id).await;
    assert!(
        matches!(finished.phase, RemotePhase::Completed),
        "{:?}",
        finished.error
    );
    let refresh = finished.refresh.unwrap();
    assert_eq!(refresh.head.unwrap().hash, new_head);
    assert!(refresh.status.unwrap().staged.is_empty());
    assert!(refresh.reset_history && refresh.clear_diff);
    assert_eq!(
        fs::read_to_string(f.b.join("file.txt")).unwrap().trim_end(),
        "pulled"
    );
    assert_eq!(tree.history(None).await.unwrap().commits[0].hash, new_head);
}

#[tokio::test]
async fn push_and_non_fast_forward_refusal() {
    let f = Fixture::new();
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    let pushed = Fixture::commit(&f.b, "from b\n", "From B");
    let started = tree.start_remote(RemoteKind::Push, None).await.unwrap();
    let finished = wait(&tree, started.id).await;
    assert!(
        matches!(finished.phase, RemotePhase::Completed),
        "{:?}",
        finished.error
    );
    assert_eq!(git(&f.bare, &["rev-parse", "refs/heads/main"]), pushed);
    git(&f.a, &["pull", "--ff-only"]);
    Fixture::commit(&f.a, "new from a\n", "Again A");
    git(&f.a, &["push"]);
    Fixture::commit(&f.b, "new from b\n", "Again B");
    let started = tree.start_remote(RemoteKind::Push, None).await.unwrap();
    let failed = wait(&tree, started.id).await;
    assert!(matches!(failed.phase, RemotePhase::Failed));
    assert!(failed.error.unwrap().message.contains("rejected"));
    let history = tree.operation_history().await;
    assert_eq!(history[0].source, OperationSource::User);
    assert_eq!(history[0].phase, OperationPhase::Failed);
    assert!(history[0]
        .diagnostics
        .as_ref()
        .unwrap()
        .contains("rejected"));
}

#[tokio::test]
async fn failed_pull_refreshes_conflict_state_without_discarding_changes() {
    let f = Fixture::new();
    Fixture::commit(&f.a, "remote edit\n", "Remote edit");
    git(&f.a, &["push"]);
    Fixture::commit(&f.b, "local edit\n", "Local edit");
    git(&f.b, &["config", "pull.rebase", "false"]);
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    let started = tree.start_remote(RemoteKind::Pull, None).await.unwrap();
    let finished = wait(&tree, started.id).await;
    assert!(matches!(finished.phase, RemotePhase::Failed));
    let refresh = finished
        .refresh
        .expect("failed pull still returns current state");
    assert!(refresh.reset_history && refresh.clear_diff);
    assert!(!refresh.status.unwrap().conflicted.is_empty());
    assert!(fs::read_to_string(f.b.join("file.txt"))
        .unwrap()
        .contains("local edit"));
}

#[tokio::test]
async fn push_without_upstream_reports_git_failure() {
    let f = Fixture::new();
    git(&f.b, &["switch", "-c", "feature"]);
    Fixture::commit(&f.b, "feature\n", "Feature");
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    let started = tree.start_remote(RemoteKind::Push, None).await.unwrap();
    let finished = wait(&tree, started.id).await;
    assert!(matches!(finished.phase, RemotePhase::Failed));
    assert_eq!(finished.error.unwrap().code, "no_upstream");
}

#[tokio::test]
async fn missing_remote_fails_without_hanging_and_switch_cancels_old_session() {
    let f = Fixture::new();
    let tree = Arc::new(WorkingTree::default());
    tree.open(f.b.to_str().unwrap()).await.unwrap();
    assert_eq!(
        tree.start_remote(RemoteKind::Fetch, Some("unconfigured"))
            .await
            .unwrap_err()
            .code,
        "remote_missing"
    );
    git(
        &f.b,
        &[
            "remote",
            "set-url",
            "origin",
            f.root.path().join("missing.git").to_str().unwrap(),
        ],
    );
    let started = tree
        .start_remote(RemoteKind::Fetch, Some("origin"))
        .await
        .unwrap();
    let failed = wait(&tree, started.id).await;
    assert!(matches!(failed.phase, RemotePhase::Failed));
    tree.open(f.a.to_str().unwrap()).await.unwrap();
    assert_eq!(tree.remotes().await.unwrap().remotes[0].name, "origin");
}

#[tokio::test]
async fn runner_cancels_a_long_running_git_child() {
    let f = Fixture::new();
    let runner = GitRunner::default();
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let started = Instant::now();
    let task = tokio::spawn(async move {
        runner
            .run_remote(
                &f.b,
                &["-c", "alias.pause=!sleep 30", "pause"],
                receiver,
                None,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    sender.send(true).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(GitRunError::Cancelled)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn runner_timeout_terminates_a_long_running_git_child() {
    let f = Fixture::new();
    let runner = GitRunner::default();
    let (_sender, receiver) = tokio::sync::watch::channel(false);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runner.run_remote(
            &f.b,
            &["-c", "alias.pause=!sleep 30", "pause"],
            receiver,
            Some(Duration::from_millis(200)),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(GitRunError::Timeout)), "{result:?}");
}
