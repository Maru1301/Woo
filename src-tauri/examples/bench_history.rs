use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use woo_lib::{git::GitRunner, history::load_history};

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

fn fixture(commits: usize) -> TempDir {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    let mut input = Vec::with_capacity(commits * 230);
    input.extend_from_slice(b"blob\nmark :1\ndata 5\nhello\n");
    for index in 0..commits {
        let mark = index + 2;
        let subject = format!("Commit {index}");
        input.extend_from_slice(format!("commit refs/heads/main\nmark :{mark}\nauthor Bench <bench@example.com> 1700000000 +0000\ncommitter Bench <bench@example.com> 1700000000 +0000\ndata {}\n{subject}\n", subject.len()).as_bytes());
        if index > 0 {
            input.extend_from_slice(format!("from :{}\n", mark - 1).as_bytes());
        }
        input.extend_from_slice(b"M 100644 :1 file.txt\n\n");
    }
    let mut child = Command::new("git")
        .arg("fast-import")
        .current_dir(temp.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "fast-import: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    temp
}

async fn measure(label: &str, count: usize) {
    let created = Instant::now();
    let temp = fixture(count);
    println!("{label}: fixture_ms={}", created.elapsed().as_millis());
    let runner = GitRunner::with_timeout(Duration::from_secs(60));
    for page_index in 0..3 {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let mut cursor = None;
            for _ in 0..page_index {
                cursor = load_history(&runner, temp.path(), cursor.as_deref())
                    .await
                    .unwrap()
                    .0
                    .next_cursor;
            }
            let (page, timing) = load_history(&runner, temp.path(), cursor.as_deref())
                .await
                .unwrap();
            let serializing = Instant::now();
            let payload = serde_json::to_vec(&page).unwrap();
            let serialization = serializing.elapsed();
            samples.push((timing, serialization, page.commits.len(), payload.len()));
        }
        samples.sort_by_key(|sample| sample.0.total);
        let (timing, serialization, rows, payload) = &samples[2];
        println!("{label} page={} rows={} git_ms={:.3} parse_ms={:.3} graph_ms={:.3} total_ms={:.3} serialize_ms={:.3} stdout_bytes={} json_bytes={}", page_index + 1, rows, timing.git.as_secs_f64()*1000.0, timing.parse.as_secs_f64()*1000.0, timing.graph.as_secs_f64()*1000.0, timing.total.as_secs_f64()*1000.0, serialization.as_secs_f64()*1000.0, timing.output_bytes, payload);
    }
}

#[tokio::main]
async fn main() {
    measure("small", 1_000).await;
    measure("medium", 10_000).await;
}
