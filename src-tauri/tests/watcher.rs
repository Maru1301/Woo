use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
use woo_lib::{conflicts::RepositoryOperationState, working_tree::WorkingTree};

fn git(directory: &Path, args: &[&str]) -> bool {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .output()
        .unwrap();
    if !output.status.success() && !matches!(args.first(), Some(&"merge")) {
        panic!(
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output.status.success()
}

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Watcher Test"]);
    git(dir.path(), &["config", "user.email", "watcher@example.com"]);
    fs::write(dir.path().join("file.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "--", "file.txt"]);
    git(dir.path(), &["commit", "-m", "base"]);
    dir
}

#[tokio::test]
async fn external_edit_add_commit_and_refs_are_revalidated() {
    let dir = fixture();
    let tree = WorkingTree::default();
    let opened = tree.open(dir.path().to_str().unwrap()).await.unwrap();
    fs::write(dir.path().join("file.txt"), "changed\n").unwrap();
    let edited = tree
        .watch_snapshot(opened.session_id, true, false, false, None)
        .await
        .unwrap();
    assert_eq!(edited.state.unwrap().status.unstaged[0].path, "file.txt");
    git(dir.path(), &["add", "--", "file.txt"]);
    let staged = tree
        .watch_snapshot(opened.session_id, true, false, false, None)
        .await
        .unwrap();
    assert_eq!(staged.state.unwrap().status.staged[0].path, "file.txt");
    git(dir.path(), &["commit", "-m", "external"]);
    let committed = tree
        .watch_snapshot(
            opened.session_id,
            false,
            true,
            true,
            opened.head.as_ref().map(|head| head.hash.as_str()),
        )
        .await
        .unwrap();
    assert!(committed.state.unwrap().status.staged.is_empty());
    assert_ne!(
        committed.identity.unwrap().1.unwrap().hash,
        opened.head.unwrap().hash
    );
    git(dir.path(), &["branch", "feature/test"]);
    git(dir.path(), &["tag", "v1"]);
    let refs = tree
        .watch_snapshot(opened.session_id, false, true, true, None)
        .await
        .unwrap();
    assert!(refs
        .branches
        .unwrap()
        .branches
        .iter()
        .any(|branch| branch.name == "feature/test"));
}

#[tokio::test]
async fn external_checkout_and_conflicted_merge_reconstruct_then_clear() {
    let dir = fixture();
    git(dir.path(), &["checkout", "-b", "feature"]);
    fs::write(dir.path().join("file.txt"), "feature\n").unwrap();
    git(dir.path(), &["commit", "-am", "feature"]);
    git(dir.path(), &["checkout", "main"]);
    let tree = WorkingTree::default();
    let opened = tree.open(dir.path().to_str().unwrap()).await.unwrap();
    git(dir.path(), &["checkout", "feature"]);
    let switched = tree
        .watch_snapshot(
            opened.session_id,
            true,
            true,
            true,
            opened.head.as_ref().map(|head| head.hash.as_str()),
        )
        .await
        .unwrap();
    assert_eq!(switched.identity.unwrap().0.as_deref(), Some("feature"));
    git(dir.path(), &["checkout", "main"]);
    fs::write(dir.path().join("file.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-am", "main"]);
    assert!(!git(dir.path(), &["merge", "feature"]));
    let conflict = tree
        .watch_snapshot(opened.session_id, true, false, false, None)
        .await
        .unwrap()
        .state
        .unwrap();
    assert!(matches!(
        conflict.operation,
        RepositoryOperationState::Merge { .. }
    ));
    assert_eq!(conflict.conflicts.len(), 1);
    git(dir.path(), &["merge", "--abort"]);
    let clear = tree
        .watch_snapshot(opened.session_id, true, false, false, None)
        .await
        .unwrap()
        .state
        .unwrap();
    assert!(matches!(clear.operation, RepositoryOperationState::None));
    assert!(clear.conflicts.is_empty());
}

#[tokio::test]
async fn old_session_cannot_validate_new_repository_and_missing_repository_is_reported() {
    let first = fixture();
    let second = fixture();
    let tree = WorkingTree::default();
    let old = tree.open(first.path().to_str().unwrap()).await.unwrap();
    let new = tree.open(second.path().to_str().unwrap()).await.unwrap();
    assert_ne!(old.session_id, new.session_id);
    assert_eq!(
        tree.watch_snapshot(old.session_id, true, true, true, None)
            .await
            .unwrap_err()
            .code,
        "repository_changed"
    );
    // Git metadata disappearance is surfaced instead of retaining old state.
    fs::remove_dir_all(second.path().join(".git")).unwrap();
    assert!(tree
        .watch_snapshot(new.session_id, true, true, true, None)
        .await
        .is_err());
}
