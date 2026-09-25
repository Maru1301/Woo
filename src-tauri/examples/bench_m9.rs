//! Local diagnostic only: generated refs and stashes, no network or personal repository.
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::TempDir;
use woo_lib::{git::GitRunner, stash::load_stashes, tags::load_tags};

fn git(directory: &Path, args: &[&str], input: Option<&str>) -> String {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::main]
async fn main() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    git(path, &["init", "-b", "main"], None);
    git(path, &["config", "user.name", "Benchmark"], None);
    git(
        path,
        &["config", "user.email", "benchmark@example.com"],
        None,
    );
    git(path, &["config", "core.autocrlf", "false"], None);
    fs::write(path.join("file.txt"), "base\n").unwrap();
    git(path, &["add", "file.txt"], None);
    git(path, &["commit", "-m", "Initial"], None);
    let head = git(path, &["rev-parse", "HEAD"], None);
    let runner = GitRunner::default();
    for count in [20, 200, 1000] {
        let mut updates = String::new();
        for index in 0..count {
            updates.push_str(&format!("update refs/tags/m9-{index:04} {head}\n"));
        }
        git(path, &["update-ref", "--stdin"], Some(&updates));
        let mut results = Vec::new();
        for _ in 0..3 {
            let (tags, time) = load_tags(&runner, path).await.unwrap();
            assert_eq!(tags.tags.len(), count);
            results.push((
                time.git.as_micros(),
                time.parse.as_micros(),
                time.total.as_micros(),
            ));
        }
        results.sort_unstable_by_key(|item| item.2);
        println!(
            "tags={count} median_git_us={} parse_us={} total_us={}",
            results[1].0, results[1].1, results[1].2
        );
    }
    for index in 0..10 {
        fs::write(path.join("file.txt"), format!("stash {index}\n")).unwrap();
        git(path, &["stash", "push", "-m", "M9 benchmark"], None);
    }
    let mut results = Vec::new();
    for _ in 0..3 {
        let (stashes, time) = load_stashes(&runner, path).await.unwrap();
        assert_eq!(stashes.stashes.len(), 10);
        results.push((
            time.git.as_micros(),
            time.parse.as_micros(),
            time.total.as_micros(),
        ));
    }
    results.sort_unstable_by_key(|item| item.2);
    println!(
        "stashes=10 median_git_us={} parse_us={} total_us={}",
        results[1].0, results[1].1, results[1].2
    );
}
