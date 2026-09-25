use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{branches::BranchKind, working_tree::WorkingTree};

fn git_output(directory: &Path, args: &[&str]) -> Output {
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
    let output = git_output(directory, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    fs::write(dir.path().join("file.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "--", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Main"]);
    dir
}

async fn open(dir: &TempDir) -> WorkingTree {
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    tree
}

#[tokio::test]
async fn enumerate_create_and_checkout_local_branches() {
    let dir = fixture();
    let tree = open(&dir).await;
    let refs = tree.create_branch("feature/a").await.unwrap();
    assert!(refs
        .branches
        .iter()
        .any(|b| b.name == "feature/a" && b.kind == BranchKind::Local && !b.is_current));
    tree.create_branch("feature/b").await.unwrap();
    let unicode = tree.create_branch("feature/測試").await.unwrap();
    assert!(unicode.branches.iter().any(|b| b.name == "feature/測試"));
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "main");
    let switched = tree.checkout_branch("feature/a").await.unwrap();
    assert_eq!(switched.branch.as_deref(), Some("feature/a"));
    assert!(
        switched
            .branches
            .branches
            .iter()
            .find(|b| b.name == "feature/a")
            .unwrap()
            .is_current
    );
    assert!(switched.status.staged.is_empty());
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "feature/a");
}

#[tokio::test]
async fn checkout_refreshes_head_and_preserves_safe_working_changes() {
    let dir = fixture();
    git(dir.path(), &["switch", "-c", "feature"]);
    fs::write(dir.path().join("file.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "--", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    git(dir.path(), &["switch", "main"]);
    fs::write(dir.path().join("untracked.txt"), "keep").unwrap();
    let tree = open(&dir).await;
    let result = tree.checkout_branch("feature").await.unwrap();
    assert_eq!(result.head.subject, "Feature");
    assert_eq!(result.status.untracked.len(), 1);
    assert_eq!(result.status.untracked[0].path, "untracked.txt");
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt"))
            .unwrap()
            .trim_end(),
        "feature"
    );
}

#[tokio::test]
async fn checkout_refuses_to_overwrite_local_changes() {
    let dir = fixture();
    git(dir.path(), &["switch", "-c", "feature"]);
    fs::write(dir.path().join("file.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "--", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Feature"]);
    git(dir.path(), &["switch", "main"]);
    fs::write(dir.path().join("file.txt"), "precious local change\n").unwrap();
    let tree = open(&dir).await;
    let error = tree.checkout_branch("feature").await.unwrap_err();
    assert_eq!(error.code, "git_failed");
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "main");
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "precious local change\n"
    );
    assert_eq!(tree.status().await.unwrap().unstaged.len(), 1);
}

#[tokio::test]
async fn detached_head_and_remote_tracking_are_distinct() {
    let dir = fixture();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/main", &head],
    );
    git(
        dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(dir.path(), &["switch", "--detach", "HEAD"]);
    let tree = open(&dir).await;
    let info = tree.open(dir.path().to_str().unwrap()).await.unwrap();
    assert!(info.branch.is_none());
    let refs = tree.branches().await.unwrap();
    assert!(refs.branches.iter().all(|b| !b.is_current));
    assert!(refs
        .branches
        .iter()
        .any(|b| b.name == "origin/main" && b.kind == BranchKind::Remote));
    assert!(!refs.branches.iter().any(|b| b.name == "origin/HEAD"));
}

#[tokio::test]
async fn invalid_names_do_not_run_git() {
    let dir = fixture();
    let tree = open(&dir).await;
    assert_eq!(
        tree.create_branch(" ").await.unwrap_err().code,
        "invalid_branch_name"
    );
    assert_eq!(
        tree.checkout_branch("bad\0name").await.unwrap_err().code,
        "invalid_branch_name"
    );
}
