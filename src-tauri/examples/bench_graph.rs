use std::time::{Duration, Instant};
use woo_lib::{
    graph::{layout_page, GraphState},
    history::{CommitInfo, PAGE_SIZE},
};

fn hash(id: usize) -> String {
    format!("{id:040x}")
}

fn commit(id: usize, parents: &[usize]) -> CommitInfo {
    CommitInfo {
        hash: hash(id),
        parent_hashes: parents.iter().map(|parent| hash(*parent)).collect(),
        author_name: String::new(),
        author_email: String::new(),
        timestamp: String::new(),
        subject: String::new(),
        refs: Vec::new(),
    }
}

fn linear(count: usize) -> Vec<CommitInfo> {
    (1..=count)
        .rev()
        .map(|id| {
            if id == 1 {
                commit(id, &[])
            } else {
                commit(id, &[id - 1])
            }
        })
        .collect()
}

fn branch_heavy(count: usize) -> Vec<CommitInfo> {
    let branches = 50;
    let depth = count / branches;
    let mut commits = Vec::with_capacity(count);
    for step in (1..=depth).rev() {
        for branch in 0..branches {
            let id = branch * (depth + 1) + step;
            commits.push(if step == 1 {
                commit(id, &[])
            } else {
                commit(id, &[id - 1])
            });
        }
    }
    commits
}

fn merge_heavy(count: usize) -> Vec<CommitInfo> {
    let mut chronological = vec![commit(1, &[])];
    let mut main = 1;
    let mut next = 2;
    while chronological.len() + 3 <= count {
        let base = main;
        chronological.push(commit(next, &[base]));
        main = next;
        next += 1;
        let side = next;
        chronological.push(commit(side, &[base]));
        next += 1;
        chronological.push(commit(next, &[main, side]));
        main = next;
        next += 1;
    }
    while chronological.len() < count {
        chronological.push(commit(next, &[main]));
        main = next;
        next += 1;
    }
    chronological.reverse();
    chronological
}

fn measure(label: &str, commits: &[CommitInfo]) {
    let mut samples = Vec::new();
    for _ in 0..5 {
        let mut state = GraphState::default();
        let mut total = Duration::ZERO;
        let mut max_page = Duration::ZERO;
        let mut first_json_bytes = 0;
        let mut first_json_time = Duration::ZERO;
        let mut max_cursor_bytes = 0;
        let mut max_lanes = 0;
        for (index, page) in commits.chunks(PAGE_SIZE).enumerate() {
            let started = Instant::now();
            let rows = layout_page(&mut state, page).unwrap();
            let elapsed = started.elapsed();
            total += elapsed;
            max_page = max_page.max(elapsed);
            max_lanes = max_lanes.max(rows.iter().map(|row| row.lane_count).max().unwrap_or(0));
            let cursor = state.encode();
            max_cursor_bytes = max_cursor_bytes.max(cursor.len());
            state = GraphState::decode(&cursor).unwrap();
            if index == 0 {
                let serializing = Instant::now();
                first_json_bytes = serde_json::to_vec(&rows).unwrap().len();
                first_json_time = serializing.elapsed();
            }
        }
        samples.push((
            total,
            max_page,
            first_json_bytes,
            first_json_time,
            max_cursor_bytes,
            max_lanes,
        ));
    }
    samples.sort_by_key(|sample| sample.0);
    let (total, max_page, bytes, json, cursor, lanes) = samples[2];
    println!("{label} commits={} layout_total_ms={:.3} max_page_ms={:.3} graph_json_bytes={} graph_json_ms={:.3} max_cursor_bytes={} max_lanes={}",
        commits.len(), total.as_secs_f64()*1000.0, max_page.as_secs_f64()*1000.0,
        bytes, json.as_secs_f64()*1000.0, cursor, lanes);
}

fn main() {
    measure("linear_1k", &linear(1_000));
    measure("linear_10k", &linear(10_000));
    measure("branch_10k", &branch_heavy(10_000));
    measure("merge_10k", &merge_heavy(10_000));
}
