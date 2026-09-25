use std::{path::Path, process::Command};
use tempfile::TempDir;
use woo_lib::{
    git::GitRunner,
    graph::{layout_page, GraphState},
    history::{load_history, PAGE_SIZE},
    working_tree::WorkingTree,
};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("Git required");
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
    git(fixture.path(), &["config", "user.name", "林明"]);
    git(
        fixture.path(),
        &["config", "user.email", "history@example.com"],
    );
    git(fixture.path(), &["config", "commit.gpgsign", "false"]);
    fixture
}

#[tokio::test]
async fn linear_pagination_root_unicode_and_refs() {
    let fixture = fixture();
    let mut hashes = Vec::new();
    for index in 0..PAGE_SIZE + 7 {
        git(
            fixture.path(),
            &[
                "commit",
                "--allow-empty",
                "-m",
                &format!("Commit {index} 🌱"),
            ],
        );
        hashes.push(git(fixture.path(), &["rev-parse", "HEAD"]));
    }
    git(fixture.path(), &["tag", "v1", &hashes[0]]);
    let tree = WorkingTree::default();
    tree.open(fixture.path().to_str().unwrap()).await.unwrap();
    let first = tree.history(None).await.unwrap();
    assert_eq!(first.commits.len(), PAGE_SIZE);
    assert_eq!(first.graph_rows.len(), PAGE_SIZE);
    assert_eq!(first.graph_rows[0].node_lane, 0);
    assert!(first.has_more);
    assert_eq!(first.commits[0].hash, hashes[PAGE_SIZE + 6]);
    assert_eq!(first.commits[0].author_name, "林明");
    assert_eq!(
        first.commits[0].subject,
        format!("Commit {} 🌱", PAGE_SIZE + 6)
    );
    assert!(first.commits[0]
        .refs
        .iter()
        .any(|name| name.contains("main")));
    let second = tree.history(first.next_cursor.as_deref()).await.unwrap();
    assert_eq!(second.commits.len(), 7);
    assert_eq!(second.graph_rows.len(), 7);
    assert_eq!(second.graph_rows.last().unwrap().parent_lanes.len(), 0);
    assert!(!second.has_more);
    assert_eq!(second.commits.last().unwrap().hash, hashes[0]);
    assert!(second.commits.last().unwrap().parent_hashes.is_empty());
    assert!(second
        .commits
        .last()
        .unwrap()
        .refs
        .iter()
        .any(|name| name.contains("v1")));
    let unique: std::collections::HashSet<_> = first
        .commits
        .iter()
        .chain(&second.commits)
        .map(|c| &c.hash)
        .collect();
    assert_eq!(unique.len(), PAGE_SIZE + 7);
    git(
        fixture.path(),
        &["commit", "--allow-empty", "-m", "New tip"],
    );
    assert_eq!(
        tree.history(first.next_cursor.as_deref())
            .await
            .unwrap_err()
            .code,
        "history_changed"
    );
}

#[tokio::test]
async fn merge_preserves_both_parents_and_empty_repository_is_empty() {
    let fixture = fixture();
    let runner = GitRunner::default();
    assert!(load_history(&runner, fixture.path(), None)
        .await
        .unwrap()
        .0
        .commits
        .is_empty());
    git(fixture.path(), &["commit", "--allow-empty", "-m", "Root"]);
    git(fixture.path(), &["switch", "-c", "feature"]);
    git(
        fixture.path(),
        &["commit", "--allow-empty", "-m", "Feature"],
    );
    let feature = git(fixture.path(), &["rev-parse", "HEAD"]);
    git(fixture.path(), &["switch", "main"]);
    git(fixture.path(), &["commit", "--allow-empty", "-m", "Main"]);
    let main = git(fixture.path(), &["rev-parse", "HEAD"]);
    git(
        fixture.path(),
        &["merge", "--no-ff", "feature", "-m", "Merge"],
    );
    let page = load_history(&runner, fixture.path(), None).await.unwrap().0;
    assert_eq!(page.commits[0].parent_hashes, vec![main, feature]);
    assert_eq!(page.graph_rows[0].parent_lanes.len(), 2);
    assert_eq!(page.commits.last().unwrap().parent_hashes.len(), 0);
}

#[tokio::test]
async fn merge_lane_continues_across_page_boundary() {
    let fixture = fixture();
    git(fixture.path(), &["commit", "--allow-empty", "-m", "Root"]);
    git(fixture.path(), &["switch", "-c", "feature"]);
    git(
        fixture.path(),
        &["commit", "--allow-empty", "-m", "Feature"],
    );
    git(fixture.path(), &["switch", "main"]);
    for index in 0..PAGE_SIZE + 5 {
        git(
            fixture.path(),
            &["commit", "--allow-empty", "-m", &format!("Main {index}")],
        );
    }
    git(
        fixture.path(),
        &["merge", "--no-ff", "feature", "-m", "Merge"],
    );
    let tree = WorkingTree::default();
    tree.open(fixture.path().to_str().unwrap()).await.unwrap();
    let first = tree.history(None).await.unwrap();
    assert_eq!(first.commits[0].parent_hashes.len(), 2);
    assert!(first.has_more);
    let second = tree.history(first.next_cursor.as_deref()).await.unwrap();
    assert!(!second.commits.is_empty());
    let actual: Vec<_> = first
        .graph_rows
        .iter()
        .chain(&second.graph_rows)
        .cloned()
        .collect();
    let mut all_commits = first.commits;
    all_commits.extend(second.commits);
    let mut continuous = GraphState::default();
    let expected = layout_page(&mut continuous, &all_commits).unwrap();
    assert_eq!(actual, expected);
    assert!(second
        .graph_rows
        .iter()
        .any(|row| row.parent_lanes.contains(&1) || row.continuations.contains(&1)));
}
