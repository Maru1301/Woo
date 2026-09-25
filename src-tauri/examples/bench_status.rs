use std::{fs, path::Path, process::Command, time::Duration};
use tempfile::TempDir;
use woo_lib::{
    git::GitRunner,
    working_tree::{load_status, StatusTiming},
};

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("Git is required");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn report(label: &str, mut samples: Vec<StatusTiming>) {
    fn summary(values: impl Iterator<Item = u128>) -> String {
        let mut values: Vec<_> = values.collect();
        values.sort_unstable();
        format!(
            "{:.3}/{:.3}/{:.3}",
            values[0] as f64 / 1000.0,
            values[values.len() / 2] as f64 / 1000.0,
            values[values.len() - 1] as f64 / 1000.0
        )
    }
    println!(
        "{label} (ms min/median/max, n={}): git={} parse={} total={}",
        samples.len(),
        summary(samples.iter().map(|sample| sample.git.as_micros())),
        summary(samples.iter().map(|sample| sample.parse.as_micros())),
        summary(samples.drain(..).map(|sample| sample.total.as_micros()))
    );
}

async fn measure(label: &str, git: &GitRunner, path: &Path) {
    let mut samples = Vec::new();
    for _ in 0..10 {
        let (_, timing) = load_status(git, path).await.expect("status failed");
        samples.push(timing);
    }
    report(label, samples);
}

#[tokio::main]
async fn main() {
    let fixture = TempDir::new().unwrap();
    let path = fixture.path();
    git(path, &["init", "-b", "main"]);
    for index in 0..1000 {
        fs::write(path.join(format!("tracked-{index:04}.txt")), b"before\n").unwrap();
    }
    git(path, &["add", "-A"]);
    git(
        path,
        &[
            "-c",
            "user.name=Bench",
            "-c",
            "user.email=bench@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Fixture",
        ],
    );
    let runner = GitRunner::with_timeout(Duration::from_secs(60));
    measure("clean (1000 tracked)", &runner, path).await;

    for index in 0..5 {
        fs::write(path.join(format!("tracked-{index:04}.txt")), b"after\n").unwrap();
        fs::write(path.join(format!("untracked-{index:04}.txt")), b"new\n").unwrap();
    }
    measure("small (5 modified, 5 untracked)", &runner, path).await;

    for index in 5..500 {
        fs::write(path.join(format!("tracked-{index:04}.txt")), b"after\n").unwrap();
        fs::write(path.join(format!("untracked-{index:04}.txt")), b"new\n").unwrap();
    }
    measure("larger (500 modified, 500 untracked)", &runner, path).await;
}
