use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{
    conflicts::{ConflictKind, ConflictSide, RepositoryOperationState},
    working_tree::{MergeOutcome, WorkingTree},
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
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    git(dir.path(), &["config", "commit.gpgSign", "false"]);
    fs::write(dir.path().join("file.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Base"]);
    dir
}
fn divergent(content: &str) -> TempDir {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("file.txt"), "main change\n").unwrap();
    git(dir.path(), &["commit", "-am", "Main change"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("file.txt"), content).unwrap();
    git(dir.path(), &["commit", "-am", "Feature change"]);
    git(dir.path(), &["switch", "main"]);
    dir
}
async fn open(dir: &TempDir) -> WorkingTree {
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    tree
}

#[tokio::test]
async fn already_up_to_date_fast_forward_and_clean_merge() {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    let tree = open(&dir).await;
    let up_to_date = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert!(matches!(up_to_date.outcome, MergeOutcome::AlreadyUpToDate));
    assert!(matches!(
        up_to_date.state.operation,
        RepositoryOperationState::None
    ));
    assert!(!up_to_date.reset_history);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    let feature_head = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "main"]);
    let ff = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert!(matches!(ff.outcome, MergeOutcome::FastForward));
    assert_eq!(ff.head.hash, feature_head);
    assert!(ff.reset_history);
    assert!(ff.state.status.conflicted.is_empty());

    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("main.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "main.txt"]);
    git(dir.path(), &["commit", "-m", "Main"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    git(dir.path(), &["switch", "main"]);
    let tree = open(&dir).await;
    let result = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert!(matches!(result.outcome, MergeOutcome::CleanMerge));
    assert!(result.error.is_none());
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(
        git(dir.path(), &["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
}

#[tokio::test]
async fn conflict_restart_text_save_stage_and_complete() {
    let dir = divergent("feature change\n");
    let tree = open(&dir).await;
    let result = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert!(matches!(result.outcome, MergeOutcome::NeedsResolution));
    assert_eq!(result.state.conflicts.len(), 1);
    assert_eq!(result.state.conflicts[0].kind, ConflictKind::BothModified);
    assert!(result.state.conflicts[0].base.is_some());
    assert!(result.state.conflicts[0].ours.is_some());
    assert!(result.state.conflicts[0].theirs.is_some());
    let restarted = open(&dir).await;
    let state = restarted.repository_state().await.unwrap();
    assert!(matches!(
        state.operation,
        RepositoryOperationState::Merge { .. }
    ));
    assert_eq!(state.conflicts.len(), 1);
    assert_eq!(
        tree.conflict_content("../outside").await.unwrap_err().code,
        "invalid_path"
    );
    let content = restarted.conflict_content("file.txt").await.unwrap();
    assert_eq!(content.base.unwrap().text.as_deref(), Some("base\n"));
    assert_eq!(content.ours.unwrap().text.as_deref(), Some("main change\n"));
    assert_eq!(
        content.theirs.unwrap().text.as_deref(),
        Some("feature change\n")
    );
    let current = content.working.unwrap().text.unwrap();
    assert!(current.contains("<<<<<<<"));
    assert_eq!(
        restarted
            .save_conflict_text("file.txt", Some("stale"), "resolved\n")
            .await
            .unwrap_err()
            .code,
        "conflict_changed"
    );
    let saved = restarted
        .save_conflict_text("file.txt", Some(&current), "resolved 日本語\n")
        .await
        .unwrap();
    assert!(saved.error.is_none());
    assert_eq!(saved.state.conflicts.len(), 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "resolved 日本語\n"
    );
    let staged = restarted.stage_conflict("file.txt").await.unwrap();
    assert!(staged.error.is_none());
    assert!(staged.state.conflicts.is_empty());
    assert!(matches!(
        staged.state.operation,
        RepositoryOperationState::Merge { .. }
    ));
    let completed = restarted
        .complete_merge("Merge message 日本語\n\nDetails\n")
        .await
        .unwrap();
    assert!(matches!(completed.outcome, MergeOutcome::Completed));
    assert!(matches!(
        completed.state.operation,
        RepositoryOperationState::None
    ));
    assert!(completed.reset_history);
    assert_eq!(
        git(dir.path(), &["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    assert!(git(dir.path(), &["log", "-1", "--format=%B"]).contains("Details"));
}

#[tokio::test]
async fn whole_file_ours_theirs_and_abort() {
    for (side, expected) in [
        (ConflictSide::Ours, "main change\n"),
        (ConflictSide::Theirs, "feature change\n"),
    ] {
        let dir = divergent("feature change\n");
        let tree = open(&dir).await;
        tree.merge_branch("refs/heads/feature").await.unwrap();
        let resolved = tree.use_conflict_side("file.txt", side).await.unwrap();
        assert!(resolved.error.is_none());
        assert!(resolved.state.conflicts.is_empty());
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            expected
        );
        let completed = tree.complete_merge("Merge sides").await.unwrap();
        assert!(matches!(completed.outcome, MergeOutcome::Completed));
    }
    let dir = divergent("feature change\n");
    let before = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = open(&dir).await;
    tree.merge_branch("refs/heads/feature").await.unwrap();
    let aborted = tree.abort_merge().await.unwrap();
    assert!(matches!(aborted.outcome, MergeOutcome::Aborted));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), before);
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "main change\n"
    );
    assert!(matches!(
        aborted.state.operation,
        RepositoryOperationState::None
    ));
}

#[tokio::test]
async fn modify_delete_add_add_binary_and_refusal() {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("file.txt"), "main change\n").unwrap();
    git(dir.path(), &["commit", "-am", "Main"]);
    git(dir.path(), &["switch", "feature"]);
    git(dir.path(), &["rm", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Delete"]);
    git(dir.path(), &["switch", "main"]);
    let tree = open(&dir).await;
    let result = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert_eq!(result.state.conflicts[0].kind, ConflictKind::DeletedByThem);
    let resolved = tree
        .use_conflict_side("file.txt", ConflictSide::Theirs)
        .await
        .unwrap();
    assert!(resolved.error.is_none());
    assert!(resolved.state.conflicts.is_empty());
    assert!(!dir.path().join("file.txt").exists());
    tree.complete_merge("Resolve deletion").await.unwrap();

    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("same.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "same.txt"]);
    git(dir.path(), &["commit", "-m", "Main adds"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("same.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "same.txt"]);
    git(dir.path(), &["commit", "-m", "Feature adds"]);
    git(dir.path(), &["switch", "main"]);
    let tree = open(&dir).await;
    let result = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert_eq!(result.state.conflicts[0].kind, ConflictKind::BothAdded);
    assert!(result.state.conflicts[0].base.is_none());
    tree.abort_merge().await.unwrap();

    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("file.txt"), [0, 1, 2, 3]).unwrap();
    git(dir.path(), &["commit", "-am", "Main binary"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("file.txt"), [0, 1, 2, 4]).unwrap();
    git(dir.path(), &["commit", "-am", "Feature binary"]);
    git(dir.path(), &["switch", "main"]);
    let tree = open(&dir).await;
    tree.merge_branch("refs/heads/feature").await.unwrap();
    let content = tree.conflict_content("file.txt").await.unwrap();
    assert!(content.ours.unwrap().is_binary);
    assert!(content.theirs.unwrap().is_binary);
    let resolved = tree
        .use_conflict_side("file.txt", ConflictSide::Theirs)
        .await
        .unwrap();
    assert!(resolved.error.is_none());
    assert!(resolved.state.conflicts.is_empty());
    assert_eq!(fs::read(dir.path().join("file.txt")).unwrap(), [0, 1, 2, 4]);
    tree.complete_merge("Resolve binary").await.unwrap();

    let dir = divergent("feature change\n");
    fs::write(dir.path().join("file.txt"), "local uncommitted\n").unwrap();
    let tree = open(&dir).await;
    let result = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert!(matches!(result.outcome, MergeOutcome::Failed));
    assert!(result.error.is_some());
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "local uncommitted\n"
    );
}

#[tokio::test]
async fn externally_started_merge_is_reconstructed_and_incompatible_mutations_are_refused() {
    let dir = divergent("feature change\n");
    let external = output(dir.path(), &["merge", "--no-edit", "feature"]);
    assert!(!external.status.success());
    let tree = open(&dir).await;
    let state = tree.repository_state().await.unwrap();
    let RepositoryOperationState::Merge {
        merge_heads,
        message,
    } = state.operation
    else {
        panic!("merge marker was not reconstructed");
    };
    assert_eq!(merge_heads.len(), 1);
    assert!(!message.is_empty());
    assert_eq!(state.conflicts.len(), 1);
    assert_eq!(
        tree.checkout_branch("feature").await.unwrap_err().code,
        "merge_in_progress"
    );
    assert_eq!(
        tree.commit("Wrong workflow").await.unwrap_err().code,
        "merge_in_progress"
    );
    assert_eq!(
        tree.create_stash(None).await.unwrap_err().code,
        "merge_in_progress"
    );
    assert_eq!(
        tree.merge_branch("refs/heads/feature")
            .await
            .unwrap_err()
            .code,
        "merge_in_progress"
    );
    tree.abort_merge().await.unwrap();
}

#[tokio::test]
async fn failed_conflict_staging_keeps_index_conflicted() {
    let dir = divergent("feature change\n");
    let tree = open(&dir).await;
    tree.merge_branch("refs/heads/feature").await.unwrap();
    assert_eq!(
        tree.stage_conflict("not-conflicted.txt")
            .await
            .unwrap_err()
            .code,
        "conflict_missing"
    );
    let lock = dir.path().join(".git").join("index.lock");
    fs::write(&lock, "external lock").unwrap();
    let result = tree.stage_conflict("file.txt").await.unwrap();
    fs::remove_file(lock).unwrap();
    assert!(result.error.is_some());
    assert_eq!(result.state.conflicts.len(), 1);
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::Merge { .. }
    ));
    tree.abort_merge().await.unwrap();
}

#[tokio::test]
async fn multiple_conflicts_load_metadata_without_content_and_bound_large_text() {
    use std::time::Instant;
    let dir = fixture();
    for index in 0..10 {
        fs::write(dir.path().join(format!("file-{index}.txt")), "base\n").unwrap();
    }
    fs::write(
        dir.path().join("large.txt"),
        format!("base\n{}", "x".repeat(300_000)),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "Fixture files"]);
    git(dir.path(), &["branch", "feature"]);
    for index in 0..10 {
        fs::write(dir.path().join(format!("file-{index}.txt")), "main\n").unwrap();
    }
    fs::write(
        dir.path().join("large.txt"),
        format!("main\n{}", "x".repeat(300_000)),
    )
    .unwrap();
    git(dir.path(), &["commit", "-am", "Main edits"]);
    git(dir.path(), &["switch", "feature"]);
    for index in 0..10 {
        fs::write(dir.path().join(format!("file-{index}.txt")), "feature\n").unwrap();
    }
    fs::write(
        dir.path().join("large.txt"),
        format!("feature\n{}", "x".repeat(300_000)),
    )
    .unwrap();
    git(dir.path(), &["commit", "-am", "Feature edits"]);
    git(dir.path(), &["switch", "main"]);
    let tree = open(&dir).await;
    let merge = tree.merge_branch("refs/heads/feature").await.unwrap();
    assert_eq!(merge.state.conflicts.len(), 11);
    let started = Instant::now();
    let state = tree.repository_state().await.unwrap();
    eprintln!(
        "M10 conflict snapshot 11 files: {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(state.conflicts.len(), 11);
    let large = tree.conflict_content("large.txt").await.unwrap();
    assert!(large.ours.unwrap().oversized);
    assert!(large.theirs.unwrap().oversized);
    assert!(large.working.unwrap().oversized);
    assert_eq!(
        tree.save_conflict_text("large.txt", None, &"x".repeat(300_000))
            .await
            .unwrap_err()
            .code,
        "conflict_too_large"
    );
}

#[tokio::test]
async fn merge_completion_respects_hooks_and_keeps_merge_state_on_rejection() {
    let dir = divergent("feature change\n");
    let tree = open(&dir).await;
    tree.merge_branch("refs/heads/feature").await.unwrap();
    tree.use_conflict_side("file.txt", ConflictSide::Ours)
        .await
        .unwrap();
    let hook = dir.path().join(".git").join("hooks").join("pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'merge blocked by hook' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let rejected = tree.complete_merge("Attempt merge").await.unwrap();
    assert!(matches!(rejected.outcome, MergeOutcome::NeedsCompletion));
    assert!(rejected
        .error
        .unwrap()
        .message
        .contains("merge blocked by hook"));
    assert!(matches!(
        rejected.state.operation,
        RepositoryOperationState::Merge { .. }
    ));
    fs::remove_file(hook).unwrap();
    let completed = tree.complete_merge("Merge after hook").await.unwrap();
    assert!(matches!(completed.outcome, MergeOutcome::Completed));
}
