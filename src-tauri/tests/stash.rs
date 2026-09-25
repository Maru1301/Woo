use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::working_tree::WorkingTree;

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
    fs::write(dir.path().join("file.txt"), "initial\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Initial"]);
    dir
}

#[tokio::test]
async fn create_apply_pop_and_drop_preserve_untracked_and_messages() {
    let dir = fixture();
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    assert!(tree.stashes().await.unwrap().stashes.is_empty());
    assert_eq!(
        tree.create_stash(None).await.unwrap_err().code,
        "no_changes_to_stash"
    );
    fs::write(dir.path().join("untracked.txt"), "keep\n").unwrap();
    fs::write(dir.path().join("file.txt"), "first change\n").unwrap();
    let created = tree.create_stash(Some("Work 日本語")).await.unwrap();
    assert!(created.error.is_none());
    assert_eq!(created.stashes.stashes.len(), 1);
    assert!(created.stashes.stashes[0].message.contains("Work 日本語"));
    assert!(created
        .status
        .unwrap()
        .untracked
        .iter()
        .any(|file| file.path == "untracked.txt"));
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "initial\n"
    );
    let hash = created.stashes.stashes[0].commit_hash.clone();
    let applied = tree.apply_stash(&hash).await.unwrap();
    assert!(applied.error.is_none());
    assert_eq!(applied.stashes.stashes.len(), 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "first change\n"
    );
    git(dir.path(), &["checkout", "--", "file.txt"]);
    let popped = tree.pop_stash(&hash).await.unwrap();
    assert!(popped.error.is_none());
    assert!(popped.stashes.stashes.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "first change\n"
    );
    git(dir.path(), &["checkout", "--", "file.txt"]);
    fs::write(dir.path().join("file.txt"), "second change\n").unwrap();
    let created = tree.create_stash(None).await.unwrap();
    let hash = created.stashes.stashes[0].commit_hash.clone();
    let dropped = tree.drop_stash(&hash).await.unwrap();
    assert!(dropped.error.is_none());
    assert!(dropped.stashes.stashes.is_empty());
}

#[tokio::test]
async fn stable_hash_selects_correct_stash_after_index_shift() {
    let dir = fixture();
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    fs::write(dir.path().join("file.txt"), "older\n").unwrap();
    let older = tree
        .create_stash(Some("older"))
        .await
        .unwrap()
        .stashes
        .stashes[0]
        .commit_hash
        .clone();
    fs::write(dir.path().join("file.txt"), "newer\n").unwrap();
    let newer = tree
        .create_stash(Some("newer"))
        .await
        .unwrap()
        .stashes
        .stashes[0]
        .commit_hash
        .clone();
    let dropped = tree.drop_stash(&newer).await.unwrap();
    assert!(dropped.error.is_none());
    assert_eq!(dropped.stashes.stashes.len(), 1);
    assert_eq!(dropped.stashes.stashes[0].commit_hash, older);
    let popped = tree.pop_stash(&older).await.unwrap();
    assert!(popped.error.is_none());
    assert!(popped.stashes.stashes.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "older\n"
    );
    assert_eq!(
        tree.drop_stash(&newer).await.unwrap_err().code,
        "stash_missing"
    );
}

#[tokio::test]
async fn failed_apply_refreshes_conflicted_status_and_preserves_stash() {
    let dir = fixture();
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    fs::write(dir.path().join("file.txt"), "stash version\n").unwrap();
    let hash = tree.create_stash(None).await.unwrap().stashes.stashes[0]
        .commit_hash
        .clone();
    fs::write(dir.path().join("file.txt"), "committed version\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Conflicting change"]);
    let result = tree.apply_stash(&hash).await.unwrap();
    assert!(result.error.is_some());
    assert!(!result.status.unwrap().conflicted.is_empty());
    assert_eq!(result.stashes.stashes.len(), 1);
    assert!(fs::read_to_string(dir.path().join("file.txt"))
        .unwrap()
        .contains("<<<<<<<"));
}

#[tokio::test]
async fn staged_changes_are_stashed_and_failed_pop_keeps_conflicts_and_stash() {
    let dir = fixture();
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    fs::write(dir.path().join("file.txt"), "staged version\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    let created = tree.create_stash(Some("index work")).await.unwrap();
    assert!(created.error.is_none());
    assert!(created.status.unwrap().staged.is_empty());
    let hash = created.stashes.stashes[0].commit_hash.clone();
    fs::write(dir.path().join("file.txt"), "different commit\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Different"]);
    let result = tree.pop_stash(&hash).await.unwrap();
    assert!(result.error.is_some());
    assert!(!result.status.unwrap().conflicted.is_empty());
    assert_eq!(result.stashes.stashes[0].commit_hash, hash);
}
