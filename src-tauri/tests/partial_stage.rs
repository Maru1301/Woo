use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{partial_stage::PartialSelection, working_tree::WorkingTree};

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
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn fixture(name: &str, content: &str) -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Woo Partial Test"]);
    git(
        repo.path(),
        &["config", "user.email", "partial@example.com"],
    );
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    git(repo.path(), &["config", "core.autocrlf", "false"]);
    fs::write(repo.path().join(name), content).unwrap();
    git(repo.path(), &["add", "--", name]);
    git(repo.path(), &["commit", "-m", "root"]);
    repo
}

async fn opened(repo: &TempDir) -> WorkingTree {
    let tree = WorkingTree::default();
    tree.open(repo.path().to_str().unwrap()).await.unwrap();
    tree
}

async fn partial(
    tree: &WorkingTree,
    path: &str,
    staged: bool,
    hunk_index: usize,
    lines: Option<Vec<usize>>,
) -> woo_lib::working_tree::PartialStageResult {
    let status = tree.status().await.unwrap();
    let change = if staged {
        &status.staged[0]
    } else {
        &status.unstaged[0]
    };
    let diff = tree
        .working_diff(staged, false, change.clone())
        .await
        .unwrap();
    let result = tree
        .partial_stage(
            path,
            staged,
            PartialSelection {
                revision: diff.revision,
                hunk_index,
                line_indices: lines,
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    result
}

#[tokio::test]
async fn stage_and_unstage_separate_hunks_preserve_worktree() {
    let name = "sp ace-初め.txt";
    let base = (0..30).map(|i| format!("line {i}\n")).collect::<String>();
    let repo = fixture(name, &base);
    let changed = base
        .replace("line 2\n", "first edit\n")
        .replace("line 25\n", "second edit\n");
    fs::write(repo.path().join(name), &changed).unwrap();
    let tree = opened(&repo).await;
    let result = partial(&tree, name, false, 0, None).await;
    assert!(result.staged_diff.as_ref().unwrap().hunks[0]
        .lines
        .iter()
        .any(|line| line.content == "first edit"));
    assert!(result.unstaged_diff.as_ref().unwrap().hunks[0]
        .lines
        .iter()
        .any(|line| line.content == "second edit"));
    assert!(!git(repo.path(), &["diff", "--cached"]).contains("second edit"));
    assert_eq!(fs::read_to_string(repo.path().join(name)).unwrap(), changed);
    tree.stage_file(name, None).await.unwrap();
    let result = partial(&tree, name, true, 0, None).await;
    assert!(result
        .unstaged_diff
        .as_ref()
        .unwrap()
        .hunks
        .iter()
        .any(|hunk| hunk.lines.iter().any(|line| line.content == "first edit")));
    assert!(!git(repo.path(), &["diff", "--cached"]).contains("first edit"));
    assert!(git(repo.path(), &["diff", "--cached"]).contains("second edit"));
    assert_eq!(fs::read_to_string(repo.path().join(name)).unwrap(), changed);
}

#[tokio::test]
async fn selected_additions_and_deletions_move_independently() {
    let repo = fixture("text.txt", "before\nold one\nold two\nafter\n");
    let changed = "before\nnew one\nnew two\nafter\n";
    fs::write(repo.path().join("text.txt"), changed).unwrap();
    let tree = opened(&repo).await;
    let status = tree.status().await.unwrap();
    let diff = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    let hunk = &diff.hunks[0];
    let first_delete = hunk
        .lines
        .iter()
        .position(|line| line.content == "old one")
        .unwrap();
    let first_add = hunk
        .lines
        .iter()
        .position(|line| line.content == "new one")
        .unwrap();
    let result = tree
        .partial_stage(
            "text.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: Some(vec![first_delete, first_add]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(repo.path(), &["show", ":text.txt"]),
        "before\nnew one\nold two\nafter\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("text.txt")).unwrap(),
        changed
    );
    tree.stage_file("text.txt", None).await.unwrap();
    let status = tree.status().await.unwrap();
    let staged = tree
        .working_diff(true, false, status.staged[0].clone())
        .await
        .unwrap();
    let hunk = &staged.hunks[0];
    let second_delete = hunk
        .lines
        .iter()
        .position(|line| line.content == "old two")
        .unwrap();
    let second_add = hunk
        .lines
        .iter()
        .position(|line| line.content == "new two")
        .unwrap();
    let result = tree
        .partial_stage(
            "text.txt",
            true,
            PartialSelection {
                revision: staged.revision,
                hunk_index: 0,
                line_indices: Some(vec![second_delete, second_add]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(repo.path(), &["show", ":text.txt"]),
        "before\nnew one\nold two\nafter\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("text.txt")).unwrap(),
        changed
    );
}

#[tokio::test]
async fn stale_external_edit_does_not_stage_new_content() {
    let repo = fixture("file.txt", "old\n");
    fs::write(repo.path().join("file.txt"), "first\n").unwrap();
    let tree = opened(&repo).await;
    let status = tree.status().await.unwrap();
    let shown = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    fs::write(repo.path().join("file.txt"), "external\n").unwrap();
    let result = tree
        .partial_stage(
            "file.txt",
            false,
            PartialSelection {
                revision: shown.revision,
                hunk_index: 0,
                line_indices: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.error.unwrap().code, "stale_diff");
    assert!(result.unstaged_diff.unwrap().hunks[0]
        .lines
        .iter()
        .any(|line| line.content == "external"));
    assert_eq!(git(repo.path(), &["show", ":file.txt"]), "old\n");
}

#[tokio::test]
async fn unicode_whitespace_and_no_newline_hunk_are_preserved() {
    let repo = fixture("日本語 file.txt", "alpha\nold\n");
    let changed = "alpha\n 新しい\t \nlast";
    fs::write(repo.path().join("日本語 file.txt"), changed).unwrap();
    let tree = opened(&repo).await;
    let result = partial(&tree, "日本語 file.txt", false, 0, None).await;
    assert!(result.unstaged_diff.is_none());
    assert_eq!(git(repo.path(), &["show", ":日本語 file.txt"]), changed);
    assert_eq!(
        fs::read_to_string(repo.path().join("日本語 file.txt")).unwrap(),
        changed
    );
}

#[tokio::test]
async fn new_and_deleted_files_remain_whole_file_only() {
    let repo = fixture("tracked.txt", "old\n");
    let tree = opened(&repo).await;
    fs::write(repo.path().join("new.txt"), "new\n").unwrap();
    tree.stage_file("new.txt", None).await.unwrap();
    let status = tree.status().await.unwrap();
    let change = status
        .staged
        .iter()
        .find(|change| change.path == "new.txt")
        .unwrap();
    let diff = tree
        .working_diff(true, false, change.clone())
        .await
        .unwrap();
    let error = tree
        .partial_stage(
            "new.txt",
            true,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "partial_unsupported");
    fs::remove_file(repo.path().join("tracked.txt")).unwrap();
    let status = tree.status().await.unwrap();
    let change = status
        .unstaged
        .iter()
        .find(|change| change.path == "tracked.txt")
        .unwrap();
    let diff = tree
        .working_diff(false, false, change.clone())
        .await
        .unwrap();
    let error = tree
        .partial_stage(
            "tracked.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "partial_unsupported");
}

#[tokio::test]
async fn crlf_selected_lines_change_only_index() {
    let repo = fixture("crlf.txt", "top\r\nold one\r\nold two\r\nbottom\r\n");
    let changed = "top\r\nnew one\r\nnew two\r\nbottom\r\n";
    fs::write(repo.path().join("crlf.txt"), changed).unwrap();
    let tree = opened(&repo).await;
    let status = tree.status().await.unwrap();
    let diff = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    let first_old = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "old one\r")
        .unwrap();
    let first_new = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "new one\r")
        .unwrap();
    let result = tree
        .partial_stage(
            "crlf.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: Some(vec![first_old, first_new]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(repo.path(), &["show", ":crlf.txt"]),
        "top\r\nnew one\r\nold two\r\nbottom\r\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("crlf.txt")).unwrap(),
        changed
    );
}

#[tokio::test]
async fn no_newline_selected_lines_preserve_marker() {
    let repo = fixture("tail.txt", "top\nold");
    let changed = "top\nnew";
    fs::write(repo.path().join("tail.txt"), changed).unwrap();
    let tree = opened(&repo).await;
    let status = tree.status().await.unwrap();
    let diff = tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    let deleted = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "old")
        .unwrap();
    let added = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "new")
        .unwrap();
    let result = tree
        .partial_stage(
            "tail.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: Some(vec![deleted, added]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(git(repo.path(), &["show", ":tail.txt"]), changed);
    assert_eq!(
        fs::read_to_string(repo.path().join("tail.txt")).unwrap(),
        changed
    );
    let status = tree.status().await.unwrap();
    let staged = tree
        .working_diff(true, false, status.staged[0].clone())
        .await
        .unwrap();
    let deleted = staged.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "old")
        .unwrap();
    let added = staged.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "new")
        .unwrap();
    let result = tree
        .partial_stage(
            "tail.txt",
            true,
            PartialSelection {
                revision: staged.revision,
                hunk_index: 0,
                line_indices: Some(vec![deleted, added]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(git(repo.path(), &["show", ":tail.txt"]), "top\nold");
    assert_eq!(
        fs::read_to_string(repo.path().join("tail.txt")).unwrap(),
        changed
    );
}

#[tokio::test]
async fn addition_only_and_deletion_only_line_selection() {
    let added = fixture("add.txt", "top\nbottom\n");
    fs::write(added.path().join("add.txt"), "top\ninsert\nbottom\n").unwrap();
    let add_tree = opened(&added).await;
    let status = add_tree.status().await.unwrap();
    let diff = add_tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    let index = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "insert")
        .unwrap();
    let result = add_tree
        .partial_stage(
            "add.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: Some(vec![index]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(added.path(), &["show", ":add.txt"]),
        "top\ninsert\nbottom\n"
    );

    let deleted = fixture("delete.txt", "top\nremove\nbottom\n");
    fs::write(deleted.path().join("delete.txt"), "top\nbottom\n").unwrap();
    let delete_tree = opened(&deleted).await;
    let status = delete_tree.status().await.unwrap();
    let diff = delete_tree
        .working_diff(false, false, status.unstaged[0].clone())
        .await
        .unwrap();
    let index = diff.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "remove")
        .unwrap();
    let result = delete_tree
        .partial_stage(
            "delete.txt",
            false,
            PartialSelection {
                revision: diff.revision,
                hunk_index: 0,
                line_indices: Some(vec![index]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(deleted.path(), &["show", ":delete.txt"]),
        "top\nbottom\n"
    );
}

#[tokio::test]
async fn leading_dash_and_bracket_path_is_literal() {
    let name = "-odd [1].txt";
    let repo = fixture(name, "before\n");
    fs::write(repo.path().join(name), "after\n").unwrap();
    let tree = opened(&repo).await;
    let result = partial(&tree, name, false, 0, None).await;
    assert!(result.unstaged_diff.is_none());
    assert_eq!(git(repo.path(), &["show", ":-odd [1].txt"]), "after\n");
}

#[tokio::test]
async fn partial_stage_preserves_other_staged_and_unstaged_edits_in_same_file() {
    let repo = fixture("both.txt", "one\ntwo\nthree\n");
    fs::write(repo.path().join("both.txt"), "ONE\ntwo\nthree\n").unwrap();
    let tree = opened(&repo).await;
    tree.stage_file("both.txt", None).await.unwrap();
    let working = "ONE\nTWO\nthree\n";
    fs::write(repo.path().join("both.txt"), working).unwrap();
    let result = partial(&tree, "both.txt", false, 0, None).await;
    assert!(result.unstaged_diff.is_none());
    assert_eq!(git(repo.path(), &["show", ":both.txt"]), working);
    let changed_again = "ONE\nTWO\nTHREE\n";
    fs::write(repo.path().join("both.txt"), changed_again).unwrap();
    let status = tree.status().await.unwrap();
    let staged = tree
        .working_diff(true, false, status.staged[0].clone())
        .await
        .unwrap();
    let first_old = staged.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "one")
        .unwrap();
    let first_new = staged.hunks[0]
        .lines
        .iter()
        .position(|line| line.content == "ONE")
        .unwrap();
    let result = tree
        .partial_stage(
            "both.txt",
            true,
            PartialSelection {
                revision: staged.revision,
                hunk_index: 0,
                line_indices: Some(vec![first_old, first_new]),
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(repo.path(), &["show", ":both.txt"]),
        "one\nTWO\nthree\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("both.txt")).unwrap(),
        changed_again
    );
}
