use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
use woo_lib::{status::ChangeKind, working_tree::WorkingTree};

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

fn fixture() -> TempDir {
    let fixture = TempDir::new().unwrap();
    git(fixture.path(), &["init", "-b", "main"]);
    git(fixture.path(), &["config", "user.name", "Test User"]);
    git(
        fixture.path(),
        &["config", "user.email", "test@example.com"],
    );
    fixture
}

#[tokio::test]
async fn modified_stage_unstage_and_untracked() {
    let fixture = fixture();
    fs::write(fixture.path().join("tracked.txt"), "before").unwrap();
    git(fixture.path(), &["add", "--", "tracked.txt"]);
    git(fixture.path(), &["commit", "-m", "First"]);
    fs::write(fixture.path().join("tracked.txt"), "after").unwrap();
    fs::write(fixture.path().join("測試 file.txt"), "new").unwrap();

    let working_tree = WorkingTree::default();
    working_tree
        .open(fixture.path().to_str().unwrap())
        .await
        .unwrap();
    let status = working_tree.status().await.unwrap();
    assert_eq!(status.unstaged[0].kind, ChangeKind::Modified);
    assert_eq!(status.untracked[0].path, "測試 file.txt");

    let staged = working_tree.stage_file("tracked.txt", None).await.unwrap();
    assert!(staged.unstaged.is_empty());
    assert_eq!(staged.staged[0].kind, ChangeKind::Modified);
    fs::write(fixture.path().join("tracked.txt"), "after again").unwrap();
    let both = working_tree.status().await.unwrap();
    assert_eq!(both.staged[0].path, "tracked.txt");
    assert_eq!(both.unstaged[0].path, "tracked.txt");
    let unstaged = working_tree
        .unstage_file("tracked.txt", None)
        .await
        .unwrap();
    assert!(unstaged.staged.is_empty());
    assert_eq!(unstaged.unstaged[0].kind, ChangeKind::Modified);
}

#[tokio::test]
async fn stage_all_and_unstage_all_cover_deletion_rename_and_new_file() {
    let fixture = fixture();
    fs::write(fixture.path().join("modify.txt"), "before").unwrap();
    fs::write(fixture.path().join("delete.txt"), "remove").unwrap();
    fs::write(fixture.path().join("old name.txt"), "rename").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "First"]);
    fs::write(fixture.path().join("modify.txt"), "after").unwrap();
    fs::remove_file(fixture.path().join("delete.txt")).unwrap();
    fs::rename(
        fixture.path().join("old name.txt"),
        fixture.path().join("new name.txt"),
    )
    .unwrap();
    fs::write(fixture.path().join("new file.txt"), "new").unwrap();

    let working_tree = WorkingTree::default();
    working_tree
        .open(fixture.path().to_str().unwrap())
        .await
        .unwrap();
    let staged = working_tree.stage_all().await.unwrap();
    assert_eq!(staged.staged.len(), 4);
    assert!(staged
        .staged
        .iter()
        .any(|file| file.kind == ChangeKind::Deleted));
    let renamed = staged
        .staged
        .iter()
        .find(|file| file.kind == ChangeKind::Renamed)
        .unwrap();
    assert_eq!(renamed.path, "new name.txt");
    assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
    assert!(staged.unstaged.is_empty());
    assert!(staged.untracked.is_empty());

    let reset = working_tree.unstage_all().await.unwrap();
    assert!(reset.staged.is_empty());
    assert!(reset
        .unstaged
        .iter()
        .any(|file| file.kind == ChangeKind::Deleted));
    assert!(reset
        .untracked
        .iter()
        .any(|file| file.path == "new file.txt"));
}

#[tokio::test]
async fn unstage_rename_and_initial_commit_work() {
    let fixture = fixture();
    fs::write(fixture.path().join("old.txt"), "content").unwrap();
    let working_tree = WorkingTree::default();
    working_tree
        .open(fixture.path().to_str().unwrap())
        .await
        .unwrap();
    let staged = working_tree.stage_file("old.txt", None).await.unwrap();
    assert_eq!(staged.staged[0].kind, ChangeKind::Added);
    let reset = working_tree.unstage_file("old.txt", None).await.unwrap();
    assert!(reset.staged.is_empty());
    assert_eq!(reset.untracked[0].path, "old.txt");

    working_tree.stage_all().await.unwrap();
    let reset_all = working_tree.unstage_all().await.unwrap();
    assert!(reset_all.staged.is_empty());
    assert_eq!(reset_all.untracked[0].path, "old.txt");
    working_tree.stage_all().await.unwrap();
    git(fixture.path(), &["commit", "-m", "First"]);
    fs::rename(
        fixture.path().join("old.txt"),
        fixture.path().join("new.txt"),
    )
    .unwrap();
    let staged = working_tree.stage_all().await.unwrap();
    let renamed = staged
        .staged
        .iter()
        .find(|file| file.kind == ChangeKind::Renamed)
        .unwrap();
    let reset = working_tree
        .unstage_file(&renamed.path, renamed.old_path.as_deref())
        .await
        .unwrap();
    assert!(reset.staged.is_empty());
    assert!(reset.untracked.iter().any(|file| file.path == "new.txt"));
}

#[tokio::test]
async fn literal_pathspec_and_invalid_paths() {
    let fixture = fixture();
    fs::write(fixture.path().join("a[1].txt"), "literal").unwrap();
    fs::write(fixture.path().join("a1.txt"), "other").unwrap();
    let working_tree = WorkingTree::default();
    working_tree
        .open(fixture.path().to_str().unwrap())
        .await
        .unwrap();
    let staged = working_tree.stage_file("a[1].txt", None).await.unwrap();
    assert_eq!(staged.staged.len(), 1);
    assert_eq!(staged.staged[0].path, "a[1].txt");
    assert_eq!(staged.untracked[0].path, "a1.txt");
    assert_eq!(
        working_tree
            .stage_file("../escape.txt", None)
            .await
            .unwrap_err()
            .code,
        "invalid_path"
    );
}
