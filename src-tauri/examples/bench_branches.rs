use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use woo_lib::{branches::load_branches, git::GitRunner};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn fixture(count: usize) -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Bench"]);
    git(dir.path(), &["config", "user.email", "bench@example.com"]);
    fs::write(dir.path().join("file.txt"), "sample").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "Initial"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let mut process = Command::new("git")
        .args(["update-ref", "--stdin"])
        .current_dir(dir.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = String::new();
    for index in 0..count.saturating_sub(1) {
        input.push_str(&format!("update refs/heads/bench/{index:04} {head}\n"));
    }
    process
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

#[tokio::main]
async fn main() {
    let runner = GitRunner::with_timeout(Duration::from_secs(60));
    for count in [20, 200, 1_000] {
        let dir = fixture(count);
        let mut samples = Vec::new();
        for _ in 0..5 {
            let (list, timing) = load_branches(&runner, dir.path()).await.unwrap();
            samples.push((timing, list.branches.len()));
        }
        samples.sort_by_key(|sample| sample.0.total);
        let (timing, rows) = &samples[2];
        println!(
            "refs={count} rows={rows} git_ms={:.3} parse_ms={:.3} total_ms={:.3}",
            timing.git.as_secs_f64() * 1000.0,
            timing.parse.as_secs_f64() * 1000.0,
            timing.total.as_secs_f64() * 1000.0
        );
    }
    let dir = fixture(2);
    git(dir.path(), &["branch", "other"]);
    let started = Instant::now();
    git(dir.path(), &["switch", "other"]);
    println!(
        "checkout_diagnostic_ms={:.3}",
        started.elapsed().as_secs_f64() * 1000.0
    );
}
