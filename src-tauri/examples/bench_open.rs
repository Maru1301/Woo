use std::{env, path::Path, time::Instant};
use woo_lib::{git::GitRunner, repository::open};

#[tokio::main]
async fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: cargo run --example bench_open -- <repository-path>");
    let git = GitRunner::default();
    let mut samples = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        open(&git, &path).await.expect("open failed");
        samples.push(start.elapsed().as_micros());
    }
    samples.sort_unstable();
    println!(
        "Repository.Open: min={}µs median={}µs max={}µs n=10",
        samples[0], samples[5], samples[9]
    );
    let mut process_samples = Vec::new();
    for _ in 0..10 {
        let output = git
            .run(Path::new(&path), &["rev-parse", "--is-inside-work-tree"])
            .await
            .expect("Git failed to start");
        assert!(output.success());
        process_samples.push(output.duration.as_micros());
    }
    process_samples.sort_unstable();
    println!(
        "Git.ValidationProcess: min={}µs median={}µs max={}µs n=10",
        process_samples[0], process_samples[5], process_samples[9]
    );
}
