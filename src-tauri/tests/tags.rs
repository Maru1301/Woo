use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use woo_lib::{tags::TagKind, working_tree::WorkingTree};

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
    git(dir.path(), &["config", "tag.gpgSign", "false"]);
    fs::write(dir.path().join("file.txt"), "initial\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Initial"]);
    dir
}

#[tokio::test]
async fn lightweight_annotated_unicode_multiline_and_delete() {
    let dir = fixture();
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(dir.path().join("file.txt"), "next\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Second"]);
    let second = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = WorkingTree::default();
    tree.open(dir.path().to_str().unwrap()).await.unwrap();
    assert!(tree.tags().await.unwrap().tags.is_empty());
    let tags = tree
        .create_tag("v1", None, Some(&first))
        .await
        .unwrap()
        .tags;
    assert_eq!(tags.tags[0].kind, TagKind::Lightweight);
    assert_eq!(tags.tags[0].target_hash, first);
    let annotation = "Release 皜祈岫\n\nDetails with quotes \"and spaces\".\n";
    let tags = tree
        .create_tag("release/皜祈岫", Some(annotation), None)
        .await
        .unwrap()
        .tags;
    let annotated = tags
        .tags
        .iter()
        .find(|tag| tag.name == "release/皜祈岫")
        .unwrap();
    assert_eq!(annotated.kind, TagKind::Annotated);
    assert_eq!(annotated.target_hash, second);
    let object = git(dir.path(), &["cat-file", "-p", "refs/tags/release/皜祈岫"]);
    assert!(object.contains(annotation.trim_end()));
    assert_eq!(
        tree.create_tag("v1", None, None).await.unwrap_err().code,
        "git_failed"
    );
    assert_eq!(
        tree.create_tag("bad name", None, None)
            .await
            .unwrap_err()
            .code,
        "git_failed"
    );
    assert_eq!(
        tree.create_tag("empty", Some("  "), None)
            .await
            .unwrap_err()
            .code,
        "empty_tag_message"
    );
    assert_eq!(
        tree.create_tag("bad", None, Some("not-a-hash"))
            .await
            .unwrap_err()
            .code,
        "invalid_tag_target"
    );
    tree.create_tag("v2", None, None).await.unwrap();
    assert_eq!(
        tree.tags()
            .await
            .unwrap()
            .tags
            .iter()
            .filter(|tag| tag.target_hash == second)
            .count(),
        2
    );
    let deleted = tree.delete_tag("v1").await.unwrap();
    assert!(!deleted.tags.tags.iter().any(|tag| tag.name == "v1"));
    assert!(
        !output(dir.path(), &["show-ref", "--verify", "refs/tags/v1"])
            .status
            .success()
    );
}
