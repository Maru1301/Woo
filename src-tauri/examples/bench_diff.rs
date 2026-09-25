use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use woo_lib::{
    diff::load_working_diff,
    git::GitRunner,
    status::{ChangeKind, FileChange},
};

fn git(path: &Path, args: &[&str]) {
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
}

fn fixture(lines_each_side: usize) -> TempDir {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    let mut before = String::new();
    let mut after = String::new();
    for index in 0..lines_each_side {
        before.push_str(&format!(
            "before {index:05} some repeated content for a realistic line\n"
        ));
        after.push_str(&format!(
            "after  {index:05} some repeated content for a realistic line\n"
        ));
    }
    fs::write(temp.path().join("bench.txt"), before).unwrap();
    git(temp.path(), &["add", "-A"]);
    git(
        temp.path(),
        &[
            "-c",
            "user.name=Bench",
            "-c",
            "user.email=bench@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Base",
        ],
    );
    fs::write(temp.path().join("bench.txt"), after).unwrap();
    temp
}

async fn measure(label: &str, lines_each_side: usize) {
    let started = Instant::now();
    let temp = fixture(lines_each_side);
    let fixture_ms = started.elapsed().as_millis();
    let runner = GitRunner::with_timeout(Duration::from_secs(60));
    let change = FileChange {
        path: "bench.txt".into(),
        old_path: None,
        kind: ChangeKind::Modified,
    };
    let mut samples = Vec::new();
    for _ in 0..5 {
        let request_started = Instant::now();
        match load_working_diff(&runner, temp.path(), false, false, change.clone()).await {
            Ok((diff, timing)) => {
                let serialize_started = Instant::now();
                let payload = serde_json::to_vec(&diff).unwrap();
                let serialize = serialize_started.elapsed();
                let lines = diff
                    .hunks
                    .iter()
                    .map(|hunk| hunk.lines.len())
                    .sum::<usize>();
                samples.push((timing.total, format!("git_ms={:.3} parse_ms={:.3} total_ms={:.3} serialize_ms={:.3} stdout_bytes={} json_bytes={} lines={}", timing.git.as_secs_f64()*1000.0, timing.parse.as_secs_f64()*1000.0, timing.total.as_secs_f64()*1000.0, serialize.as_secs_f64()*1000.0, timing.output_bytes, payload.len(), lines)));
            }
            Err(error) => samples.push((
                request_started.elapsed(),
                format!(
                    "rejected={} total_ms={:.3}",
                    error.code,
                    request_started.elapsed().as_secs_f64() * 1000.0
                ),
            )),
        }
    }
    samples.sort_by_key(|sample| sample.0);
    println!("{label} fixture_ms={fixture_ms} {}", samples[2].1);
}

#[tokio::main]
async fn main() {
    measure("small_100_diff_lines", 50).await;
    measure("medium_10000_diff_lines", 5_000).await;
    measure("large_50000_diff_lines", 25_000).await;
}
