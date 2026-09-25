use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{git::GitRunner, working_tree::WorkingTree};

fn git_output(path: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("Git must be installed")
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = git_output(path, args);
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn fixture() -> TempDir {
    let fixture = TempDir::new().unwrap();
    git(fixture.path(), &["init", "-b", "main"]);
    git(fixture.path(), &["config", "user.name", "Woo Test"]);
    git(
        fixture.path(),
        &["config", "user.email", "woo-test@example.com"],
    );
    git(fixture.path(), &["config", "commit.gpgsign", "false"]);
    fixture
}

fn stored_message(path: &Path) -> Vec<u8> {
    let raw = git_output(path, &["cat-file", "commit", "HEAD"]);
    assert!(raw.status.success());
    let separator = raw
        .stdout
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap();
    raw.stdout[separator + 2..].to_vec()
}

async fn opened(fixture: &TempDir) -> WorkingTree {
    let tree = WorkingTree::default();
    tree.open(fixture.path().to_str().unwrap()).await.unwrap();
    tree
}

#[tokio::test]
async fn initial_commit_updates_head_and_clears_index() {
    let fixture = fixture();
    fs::write(fixture.path().join("first.txt"), "first").unwrap();
    let tree = opened(&fixture).await;
    tree.stage_file("first.txt", None).await.unwrap();
    let result = tree.commit("Initial commit").await.unwrap();
    assert_eq!(result.head.subject, "Initial commit");
    assert_eq!(
        result.head.hash,
        git(fixture.path(), &["rev-parse", "HEAD"])
    );
    assert_eq!(stored_message(fixture.path()), b"Initial commit");
    assert!(result.status.staged.is_empty());
    assert!(result.status.unstaged.is_empty());
    assert!(result.status.untracked.is_empty());
}

#[tokio::test]
async fn commit_uses_only_index_and_preserves_unstaged_and_untracked() {
    let fixture = fixture();
    fs::write(fixture.path().join("a.txt"), "old A").unwrap();
    fs::write(fixture.path().join("b.txt"), "old B").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Baseline"]);
    fs::write(fixture.path().join("a.txt"), "new A").unwrap();
    fs::write(fixture.path().join("b.txt"), "new B").unwrap();
    fs::write(fixture.path().join("c.txt"), "new C").unwrap();
    let tree = opened(&fixture).await;
    tree.stage_file("a.txt", None).await.unwrap();

    let result = tree.commit("Commit A only").await.unwrap();
    assert!(result.status.staged.is_empty());
    assert_eq!(result.status.unstaged.len(), 1);
    assert_eq!(result.status.unstaged[0].path, "b.txt");
    assert_eq!(result.status.untracked.len(), 1);
    assert_eq!(result.status.untracked[0].path, "c.txt");
    assert_eq!(git(fixture.path(), &["show", "HEAD:a.txt"]), "new A");
    assert_eq!(git(fixture.path(), &["show", "HEAD:b.txt"]), "old B");
}

#[tokio::test]
async fn validates_empty_message_and_empty_index() {
    let fixture = fixture();
    let tree = opened(&fixture).await;
    assert_eq!(
        tree.commit(" \n\t ").await.unwrap_err().code,
        "empty_commit_message"
    );
    assert_eq!(
        tree.commit("Not empty").await.unwrap_err().code,
        "no_staged_changes"
    );
    fs::write(fixture.path().join("file.txt"), "content").unwrap();
    tree.stage_all().await.unwrap();
    assert_eq!(
        tree.commit("").await.unwrap_err().code,
        "empty_commit_message"
    );
    assert!(tree.status().await.unwrap().staged.len() == 1);
}

#[tokio::test]
async fn preserves_unicode_quotes_and_multiline_message() {
    let fixture = fixture();
    fs::write(fixture.path().join("file.txt"), "content").unwrap();
    let tree = opened(&fixture).await;
    tree.stage_all().await.unwrap();
    let message = "Fix \"quoted\" path 測試\n\nSecond line with 'single quotes'\n";
    tree.commit(message).await.unwrap();
    assert_eq!(stored_message(fixture.path()), message.as_bytes());
}

#[tokio::test]
async fn failing_pre_commit_hook_rejects_commit() {
    let fixture = fixture();
    fs::write(fixture.path().join("file.txt"), "content").unwrap();
    let tree = opened(&fixture).await;
    tree.stage_all().await.unwrap();
    let hook = fixture.path().join(".git").join("hooks").join("pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'blocked by Woo test hook' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let error = tree.commit("Should fail").await.unwrap_err();
    assert_eq!(error.code, "git_failed");
    assert!(error.message.contains("blocked by Woo test hook"));
    assert!(
        !git_output(fixture.path(), &["rev-parse", "--verify", "HEAD"])
            .status
            .success()
    );
    assert_eq!(tree.status().await.unwrap().staged.len(), 1);
}

#[tokio::test]
async fn early_git_failure_keeps_command_diagnostics() {
    let fixture = fixture();
    let message = vec![b'x'; 1024 * 1024];
    let output = GitRunner::default()
        .run_with_input(fixture.path(), &["not-a-git-command"], Some(&message))
        .await
        .unwrap();
    assert!(!output.success());
    assert!(!output.stderr.is_empty());
}
