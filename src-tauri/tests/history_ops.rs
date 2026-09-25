use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{
    conflicts::{ConflictSide, RepositoryOperationState},
    working_tree::{ResetMode, WorkingTree},
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
async fn open(dir: &TempDir) -> WorkingTree {
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    tree
}
fn divergent() -> (TempDir, String, String) {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("file.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-am", "Main"]);
    let main = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("file.txt"), "feature\n").unwrap();
    git(dir.path(), &["commit", "-am", "Feature"]);
    let feature = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, main, feature)
}

#[tokio::test]
async fn rebase_success_conflict_restart_continue_skip_abort_and_external_detection() {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("main.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "main.txt"]);
    git(dir.path(), &["commit", "-m", "Main"]);
    let main = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    let tree = open(&dir).await;
    let result = tree.rebase_onto("refs/heads/main").await.unwrap();
    assert!(result.error.is_none());
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD^"]), main);
    assert!(result.reset_history);
    let no_op = tree.rebase_onto("refs/heads/main").await.unwrap();
    assert!(no_op.error.is_none());
    assert!(!no_op.reset_history);
    assert!(matches!(
        no_op.state.operation,
        RepositoryOperationState::None
    ));

    for outcome in ["continue", "skip", "abort", "external"] {
        let (dir, main, feature) = divergent();
        let tree = open(&dir).await;
        let result = if outcome == "external" {
            let output = output(dir.path(), &["rebase", "main"]);
            assert!(!output.status.success());
            tree.repository_state().await.unwrap()
        } else {
            tree.rebase_onto("refs/heads/main").await.unwrap().state
        };
        assert!(matches!(
            result.operation,
            RepositoryOperationState::Rebase { .. }
        ));
        assert_eq!(result.conflicts.len(), 1);
        let restarted = open(&dir).await;
        let state = restarted.repository_state().await.unwrap();
        assert!(matches!(
            state.operation,
            RepositoryOperationState::Rebase { .. }
        ));
        assert_eq!(
            restarted
                .reset_to(&main, ResetMode::Hard)
                .await
                .unwrap_err()
                .code,
            "operation_in_progress"
        );
        assert_eq!(
            restarted.checkout_branch("main").await.unwrap_err().code,
            "operation_in_progress"
        );
        let content = restarted.conflict_content("file.txt").await.unwrap();
        assert_eq!(content.ours.unwrap().text.as_deref(), Some("main\n"));
        assert_eq!(content.theirs.unwrap().text.as_deref(), Some("feature\n"));
        match outcome {
            "continue" => {
                restarted
                    .use_conflict_side("file.txt", ConflictSide::Theirs)
                    .await
                    .unwrap();
                let result = restarted.continue_history_operation().await.unwrap();
                assert!(result.error.is_none(), "{:?}", result.error);
                assert!(matches!(
                    result.state.operation,
                    RepositoryOperationState::None
                ));
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD^"]), main);
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "feature\n"
                );
            }
            "skip" => {
                let result = restarted.skip_history_operation().await.unwrap();
                assert!(result.error.is_none());
                assert!(matches!(
                    result.state.operation,
                    RepositoryOperationState::None
                ));
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), main);
            }
            _ => {
                let result = restarted.abort_history_operation().await.unwrap();
                assert!(result.error.is_none());
                assert!(matches!(
                    result.state.operation,
                    RepositoryOperationState::None
                ));
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), feature);
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "feature\n"
                );
            }
        }
    }
}

#[tokio::test]
async fn cherry_pick_success_conflict_continue_abort_and_restart() {
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    let feature = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "main"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = open(&dir).await;
    let result = tree.cherry_pick(&feature).await.unwrap();
    assert!(result.error.is_none());
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("feature.txt")).unwrap(),
        "feature\n"
    );
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD^"]), base);

    for action in ["continue", "abort", "external"] {
        let (dir, main, feature) = divergent();
        git(dir.path(), &["switch", "main"]);
        let tree = open(&dir).await;
        if action == "external" {
            assert!(!output(dir.path(), &["cherry-pick", &feature])
                .status
                .success());
        } else {
            let result = tree.cherry_pick(&feature).await.unwrap();
            assert!(result.error.is_some());
        }
        let restarted = open(&dir).await;
        assert!(matches!(
            restarted.repository_state().await.unwrap().operation,
            RepositoryOperationState::CherryPick { .. }
        ));
        if action == "continue" {
            restarted
                .use_conflict_side("file.txt", ConflictSide::Theirs)
                .await
                .unwrap();
            let result = restarted.continue_history_operation().await.unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            assert!(matches!(
                result.state.operation,
                RepositoryOperationState::None
            ));
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                "feature\n"
            );
        } else {
            let result = restarted.abort_history_operation().await.unwrap();
            assert!(result.error.is_none());
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), main);
        }
    }
}

#[tokio::test]
async fn revert_success_conflict_continue_abort_restart_and_merge_refusal() {
    let dir = fixture();
    fs::write(dir.path().join("file.txt"), "first\n").unwrap();
    git(dir.path(), &["commit", "-am", "First"]);
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = open(&dir).await;
    let success = tree.revert_commit(&first).await.unwrap();
    assert!(success.error.is_none());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "base\n"
    );
    for action in ["continue", "abort", "external"] {
        let dir = fixture();
        fs::write(dir.path().join("file.txt"), "first\n").unwrap();
        git(dir.path(), &["commit", "-am", "First"]);
        let first = git(dir.path(), &["rev-parse", "HEAD"]);
        fs::write(dir.path().join("file.txt"), "second\n").unwrap();
        git(dir.path(), &["commit", "-am", "Second"]);
        let second = git(dir.path(), &["rev-parse", "HEAD"]);
        let tree = open(&dir).await;
        if action == "external" {
            assert!(!output(dir.path(), &["revert", "--no-edit", &first])
                .status
                .success());
        } else {
            assert!(tree.revert_commit(&first).await.unwrap().error.is_some());
        }
        let restarted = open(&dir).await;
        assert!(matches!(
            restarted.repository_state().await.unwrap().operation,
            RepositoryOperationState::Revert { .. }
        ));
        if action == "continue" {
            let current = restarted
                .conflict_content("file.txt")
                .await
                .unwrap()
                .working
                .unwrap()
                .text
                .unwrap();
            restarted
                .save_conflict_text("file.txt", Some(&current), "resolved\n")
                .await
                .unwrap();
            restarted.stage_conflict("file.txt").await.unwrap();
            let result = restarted.continue_history_operation().await.unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            assert!(matches!(
                result.state.operation,
                RepositoryOperationState::None
            ));
            assert_eq!(
                fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                "resolved\n"
            );
        } else {
            let result = restarted.abort_history_operation().await.unwrap();
            assert!(result.error.is_none());
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), second);
        }
    }
    let dir = fixture();
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("main.txt"), "m\n").unwrap();
    git(dir.path(), &["add", "main.txt"]);
    git(dir.path(), &["commit", "-m", "Main"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("feature.txt"), "f\n").unwrap();
    git(dir.path(), &["add", "feature.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    git(dir.path(), &["switch", "main"]);
    git(dir.path(), &["merge", "--no-edit", "feature"]);
    let merge = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = open(&dir).await;
    assert_eq!(
        tree.revert_commit(&merge).await.unwrap_err().code,
        "merge_commit_unsupported"
    );
}

#[tokio::test]
async fn reset_modes_preserve_or_discard_expected_data_and_protect_untracked() {
    for mode in [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard] {
        let dir = fixture();
        let base = git(dir.path(), &["rev-parse", "HEAD"]);
        fs::write(dir.path().join("file.txt"), "committed\n").unwrap();
        git(dir.path(), &["commit", "-am", "Committed"]);
        fs::write(dir.path().join("file.txt"), "working\n").unwrap();
        fs::write(dir.path().join("staged.txt"), "staged\n").unwrap();
        git(dir.path(), &["add", "staged.txt"]);
        fs::write(dir.path().join("untracked.txt"), "safe\n").unwrap();
        let tree = open(&dir).await;
        let result = tree.reset_to(&base, mode).await.unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), base);
        assert_eq!(
            fs::read_to_string(dir.path().join("untracked.txt")).unwrap(),
            "safe\n"
        );
        match mode {
            ResetMode::Soft => {
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "working\n"
                );
                assert!(result
                    .state
                    .status
                    .staged
                    .iter()
                    .any(|f| f.path == "file.txt"));
                assert!(result
                    .state
                    .status
                    .staged
                    .iter()
                    .any(|f| f.path == "staged.txt"));
            }
            ResetMode::Mixed => {
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "working\n"
                );
                assert!(result.state.status.staged.is_empty());
                assert!(result
                    .state
                    .status
                    .untracked
                    .iter()
                    .any(|f| f.path == "staged.txt"));
            }
            ResetMode::Hard => {
                assert_eq!(
                    fs::read_to_string(dir.path().join("file.txt")).unwrap(),
                    "base\n"
                );
                assert!(result.state.status.staged.is_empty());
                assert!(!dir.path().join("staged.txt").exists());
            }
        }
    }
    let dir = fixture();
    fs::write(dir.path().join("collision.txt"), "tracked\n").unwrap();
    git(dir.path(), &["add", "collision.txt"]);
    git(dir.path(), &["commit", "-m", "Add"]);
    let target = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["rm", "collision.txt"]);
    git(dir.path(), &["commit", "-m", "Delete"]);
    let before = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(dir.path().join("collision.txt"), "untracked\n").unwrap();
    let tree = open(&dir).await;
    assert_eq!(
        tree.reset_to(&target, ResetMode::Hard)
            .await
            .unwrap_err()
            .code,
        "untracked_would_be_overwritten"
    );
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), before);
    assert_eq!(
        fs::read_to_string(dir.path().join("collision.txt")).unwrap(),
        "untracked\n"
    );
}

#[tokio::test]
async fn empty_cherry_pick_can_be_skipped_and_ignored_collision_is_refused() {
    let dir = fixture();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = open(&dir).await;
    let result = tree.cherry_pick(&head).await.unwrap();
    assert!(result.error.is_some());
    assert!(matches!(
        result.state.operation,
        RepositoryOperationState::CherryPick { .. }
    ));
    assert!(result.state.conflicts.is_empty());
    let skipped = tree.skip_history_operation().await.unwrap();
    assert!(skipped.error.is_none(), "{:?}", skipped.error);
    assert!(matches!(
        skipped.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);

    let dir = fixture();
    fs::write(dir.path().join("ignored.txt"), "tracked\n").unwrap();
    git(dir.path(), &["add", "ignored.txt"]);
    git(dir.path(), &["commit", "-m", "Add ignored path"]);
    let target = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["rm", "ignored.txt"]);
    git(dir.path(), &["commit", "-m", "Remove ignored path"]);
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "ignored content\n").unwrap();
    let tree = open(&dir).await;
    assert_eq!(
        tree.reset_to(&target, ResetMode::Hard)
            .await
            .unwrap_err()
            .code,
        "untracked_would_be_overwritten"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("ignored.txt")).unwrap(),
        "ignored content\n"
    );
}

#[tokio::test]
async fn apply_backend_rebase_is_reconstructed() {
    let (dir, _, _) = divergent();
    let result = output(
        dir.path(),
        &["-c", "rebase.backend=apply", "rebase", "main"],
    );
    assert!(!result.status.success());
    assert!(dir.path().join(".git/rebase-apply").is_dir());
    assert!(dir.path().join(".git/rebase-apply/rebasing").is_file());
    let restarted = open(&dir).await;
    let state = restarted.repository_state().await.unwrap();
    assert!(matches!(
        state.operation,
        RepositoryOperationState::Rebase { .. }
    ));
    assert_eq!(state.conflicts.len(), 1);
    let aborted = restarted.abort_history_operation().await.unwrap();
    assert!(aborted.error.is_none());
    assert!(matches!(
        aborted.state.operation,
        RepositoryOperationState::None
    ));
}

#[tokio::test]
async fn git_am_directory_is_not_misidentified_as_rebase() {
    let dir = fixture();
    fs::create_dir(dir.path().join(".git/rebase-apply")).unwrap();
    let tree = open(&dir).await;
    assert_eq!(
        tree.repository_state().await.unwrap_err().code,
        "unsupported_operation"
    );
}

#[tokio::test]
async fn detached_head_refuses_history_mutations() {
    let dir = fixture();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "--detach", "HEAD"]);
    let tree = open(&dir).await;
    assert_eq!(
        tree.cherry_pick(&head).await.unwrap_err().code,
        "detached_head"
    );
    assert_eq!(
        tree.revert_commit(&head).await.unwrap_err().code,
        "detached_head"
    );
    assert_eq!(
        tree.reset_to(&head, ResetMode::Mixed)
            .await
            .unwrap_err()
            .code,
        "detached_head"
    );
}

#[tokio::test]
async fn rebase_continue_can_stop_at_another_conflicting_commit() {
    let dir = fixture();
    fs::write(dir.path().join("other.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "other.txt"]);
    git(dir.path(), &["commit", "-m", "Other base"]);
    git(dir.path(), &["branch", "feature"]);
    fs::write(dir.path().join("file.txt"), "main file\n").unwrap();
    fs::write(dir.path().join("other.txt"), "main other\n").unwrap();
    git(dir.path(), &["commit", "-am", "Main both"]);
    let main = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "feature"]);
    fs::write(dir.path().join("file.txt"), "feature file\n").unwrap();
    git(dir.path(), &["commit", "-am", "Feature file"]);
    fs::write(dir.path().join("other.txt"), "feature other\n").unwrap();
    git(dir.path(), &["commit", "-am", "Feature other"]);
    let tree = open(&dir).await;
    let first = tree.rebase_onto("refs/heads/main").await.unwrap();
    assert!(matches!(
        first.state.operation,
        RepositoryOperationState::Rebase { .. }
    ));
    assert_eq!(first.state.conflicts[0].path, "file.txt");
    tree.use_conflict_side("file.txt", ConflictSide::Theirs)
        .await
        .unwrap();
    let second = tree.continue_history_operation().await.unwrap();
    assert!(matches!(
        second.state.operation,
        RepositoryOperationState::Rebase { .. }
    ));
    assert_eq!(second.state.conflicts[0].path, "other.txt");
    tree.use_conflict_side("other.txt", ConflictSide::Theirs)
        .await
        .unwrap();
    let finished = tree.continue_history_operation().await.unwrap();
    assert!(finished.error.is_none(), "{:?}", finished.error);
    assert!(matches!(
        finished.state.operation,
        RepositoryOperationState::None
    ));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD~2"]), main);
}
