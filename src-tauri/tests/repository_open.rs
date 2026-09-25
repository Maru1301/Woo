use std::{path::Path, process::Command};
use tempfile::TempDir;
use woo_lib::{git::GitRunner, repository::open};

fn git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn opens_committed_repository_and_subdirectory() {
    let fixture = TempDir::new().unwrap();
    git(fixture.path(), &["init", "-b", "main"]);
    git(fixture.path(), &["config", "user.name", "Test User"]);
    git(
        fixture.path(),
        &["config", "user.email", "test@example.com"],
    );
    std::fs::write(fixture.path().join("readme.txt"), "hello").unwrap();
    git(fixture.path(), &["add", "readme.txt"]);
    git(fixture.path(), &["commit", "-m", "Initial commit"]);
    let nested = fixture.path().join("nested");
    std::fs::create_dir(&nested).unwrap();

    let info = open(&GitRunner::default(), nested.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(info.branch.as_deref(), Some("main"));
    assert_eq!(info.head.as_ref().unwrap().subject, "Initial commit");
    assert_eq!(
        Path::new(&info.path).canonicalize().unwrap(),
        fixture.path().canonicalize().unwrap()
    );
}

#[tokio::test]
async fn handles_unborn_and_detached_head() {
    let fixture = TempDir::new().unwrap();
    git(fixture.path(), &["init", "-b", "main"]);
    let unborn = open(&GitRunner::default(), fixture.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(unborn.branch.as_deref(), Some("main"));
    assert!(unborn.head.is_none());

    git(fixture.path(), &["config", "user.name", "Test User"]);
    git(
        fixture.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(fixture.path(), &["commit", "--allow-empty", "-m", "First"]);
    git(fixture.path(), &["checkout", "--detach", "HEAD"]);
    let detached = open(&GitRunner::default(), fixture.path().to_str().unwrap())
        .await
        .unwrap();
    assert!(detached.branch.is_none());
    assert_eq!(detached.head.unwrap().subject, "First");
}

#[tokio::test]
async fn rejects_non_repository() {
    let fixture = TempDir::new().unwrap();
    let error = open(&GitRunner::default(), fixture.path().to_str().unwrap())
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalid_repository");
}
