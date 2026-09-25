use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{
    diff::{load_commit_diff, load_commit_files, DiffLineKind},
    git::GitRunner,
    status::ChangeKind,
    working_tree::WorkingTree,
};

fn git_output(path: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("Git required")
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = git_output(path, args);
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn fixture() -> TempDir {
    let fixture = TempDir::new().unwrap();
    git(fixture.path(), &["init", "-b", "main"]);
    git(fixture.path(), &["config", "user.name", "Woo Diff Test"]);
    git(
        fixture.path(),
        &["config", "user.email", "diff@example.com"],
    );
    git(fixture.path(), &["config", "commit.gpgsign", "false"]);
    fixture
}

#[tokio::test]
async fn working_unstaged_staged_untracked_and_initial_index() {
    let fixture = fixture();
    fs::write(fixture.path().join("初め file.txt"), "first\n").unwrap();
    let tree = WorkingTree::default();
    tree.open(fixture.path().to_str().unwrap()).await.unwrap();
    let staged = tree.stage_file("初め file.txt", None).await.unwrap();
    let initial = tree
        .working_diff(true, false, staged.staged[0].clone())
        .await
        .unwrap();
    assert_eq!(initial.hunks[0].lines[0].content, "first");
    git(fixture.path(), &["commit", "-m", "Root"]);
    fs::write(fixture.path().join("初め file.txt"), "second 🌱\n").unwrap();
    let status = tree.status().await.unwrap();
    let unstaged = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    assert_eq!(unstaged.hunks[0].lines[1].content, "second 🌱");
    assert_eq!(unstaged.hunks[0].lines[1].new_line_number, Some(1));
    let staged = tree.stage_file("初め file.txt", None).await.unwrap();
    let staged_diff = tree
        .working_diff(true, false, staged.staged[0].clone())
        .await
        .unwrap();
    assert!(matches!(
        staged_diff.hunks[0].lines[1].kind,
        DiffLineKind::Addition
    ));
    fs::write(fixture.path().join("-odd [1].txt"), "new file\n").unwrap();
    let status = tree.status().await.unwrap();
    let new_file = status
        .untracked
        .iter()
        .find(|file| file.path == "-odd [1].txt")
        .unwrap();
    let untracked = tree
        .working_diff(false, true, new_file.clone())
        .await
        .unwrap();
    assert_eq!(untracked.hunks[0].lines[0].content, "new file");
    assert_eq!(untracked.hunks[0].lines[0].new_line_number, Some(1));
    fs::remove_file(fixture.path().join("初め file.txt")).unwrap();
    let status = tree.status().await.unwrap();
    let deleted = status
        .unstaged
        .iter()
        .find(|file| file.kind == ChangeKind::Deleted)
        .unwrap();
    let deletion = tree
        .working_diff(false, false, deleted.clone())
        .await
        .unwrap();
    assert!(matches!(
        deletion.hunks[0].lines[0].kind,
        DiffLineKind::Deletion
    ));
}

#[tokio::test]
async fn commit_root_normal_merge_rename_and_binary() {
    let fixture = fixture();
    fs::write(fixture.path().join("old name.txt"), "before\n").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Root"]);
    let root = git(fixture.path(), &["rev-parse", "HEAD"]);
    let runner = GitRunner::default();
    let root_files = load_commit_files(&runner, fixture.path(), &root)
        .await
        .unwrap();
    assert_eq!(root_files[0].kind, ChangeKind::Added);
    assert_eq!(
        load_commit_diff(&runner, fixture.path(), &root, root_files[0].clone())
            .await
            .unwrap()
            .0
            .hunks[0]
            .lines[0]
            .content,
        "before"
    );

    fs::write(fixture.path().join("old name.txt"), "after\n").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Normal"]);
    let normal = git(fixture.path(), &["rev-parse", "HEAD"]);
    let normal_files = load_commit_files(&runner, fixture.path(), &normal)
        .await
        .unwrap();
    assert_eq!(normal_files[0].kind, ChangeKind::Modified);
    assert_eq!(
        load_commit_diff(&runner, fixture.path(), &normal, normal_files[0].clone())
            .await
            .unwrap()
            .0
            .hunks[0]
            .lines[1]
            .content,
        "after"
    );

    git(fixture.path(), &["switch", "-c", "feature"]);
    fs::write(fixture.path().join("feature.txt"), "feature\n").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Feature"]);
    git(fixture.path(), &["switch", "main"]);
    fs::write(fixture.path().join("main.txt"), "main\n").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Main"]);
    git(
        fixture.path(),
        &["merge", "--no-ff", "feature", "-m", "Merge"],
    );
    let merge = git(fixture.path(), &["rev-parse", "HEAD"]);
    let merge_files = load_commit_files(&runner, fixture.path(), &merge)
        .await
        .unwrap();
    assert!(merge_files.iter().any(|file| file.path == "feature.txt"));
    assert!(!merge_files.iter().any(|file| file.path == "main.txt"));
    let feature = merge_files
        .iter()
        .find(|file| file.path == "feature.txt")
        .unwrap();
    assert_eq!(
        load_commit_diff(&runner, fixture.path(), &merge, feature.clone())
            .await
            .unwrap()
            .0
            .hunks[0]
            .lines[0]
            .content,
        "feature"
    );

    fs::rename(
        fixture.path().join("old name.txt"),
        fixture.path().join("new name.txt"),
    )
    .unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Rename"]);
    let rename = git(fixture.path(), &["rev-parse", "HEAD"]);
    let renamed = load_commit_files(&runner, fixture.path(), &rename)
        .await
        .unwrap();
    assert_eq!(renamed[0].kind, ChangeKind::Renamed);
    assert_eq!(renamed[0].old_path.as_deref(), Some("old name.txt"));
    assert_eq!(renamed[0].path, "new name.txt");
    assert!(
        load_commit_diff(&runner, fixture.path(), &rename, renamed[0].clone())
            .await
            .unwrap()
            .0
            .hunks
            .is_empty()
    );

    fs::write(fixture.path().join("binary.dat"), b"before\0after").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Binary"]);
    let binary = git(fixture.path(), &["rev-parse", "HEAD"]);
    let binary_files = load_commit_files(&runner, fixture.path(), &binary)
        .await
        .unwrap();
    assert!(
        load_commit_diff(&runner, fixture.path(), &binary, binary_files[0].clone())
            .await
            .unwrap()
            .0
            .is_binary
    );
}

#[tokio::test]
async fn large_patch_is_rejected_before_full_buffering() {
    let fixture = fixture();
    fs::write(fixture.path().join("large.txt"), "small\n").unwrap();
    git(fixture.path(), &["add", "-A"]);
    git(fixture.path(), &["commit", "-m", "Root"]);
    fs::write(
        fixture.path().join("large.txt"),
        vec![b'x'; 3 * 1024 * 1024],
    )
    .unwrap();
    let tree = WorkingTree::default();
    tree.open(fixture.path().to_str().unwrap()).await.unwrap();
    let status = tree.status().await.unwrap();
    let error = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, "diff_too_large");
}
