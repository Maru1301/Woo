use crate::{
    error::{git_failure, AppError},
    git::GitRunner,
    graph::{layout_page, GraphRow, GraphState},
};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub const PAGE_SIZE: usize = 100;
const FORMAT: &str = "%H%x00%P%x00%an%x00%ae%x00%aI%x00%s%x00%D";

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub hash: String,
    pub parent_hashes: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: String,
    pub subject: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHistoryPage {
    pub commits: Vec<CommitInfo>,
    pub graph_rows: Vec<GraphRow>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug)]
pub struct HistoryTiming {
    pub git: Duration,
    pub parse: Duration,
    pub graph: Duration,
    pub total: Duration,
    pub output_bytes: usize,
}

fn malformed() -> AppError {
    AppError::new(
        "invalid_git_output",
        "Git returned malformed commit history data.",
    )
}

fn text(bytes: &[u8]) -> Result<String, AppError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        AppError::new(
            "invalid_git_output",
            "Git returned non-UTF-8 commit history data.",
        )
    })
}

fn hash(bytes: &[u8]) -> Result<String, AppError> {
    if !(bytes.len() == 40 || bytes.len() == 64) || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(malformed());
    }
    text(bytes)
}

fn parse_refs(bytes: &[u8]) -> Result<Vec<String>, AppError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut refs = Vec::new();
    let mut remainder = bytes;
    while let Some(index) = remainder.windows(2).position(|pair| pair == b", ") {
        refs.push(text(&remainder[..index])?);
        remainder = &remainder[index + 2..];
    }
    refs.push(text(remainder)?);
    Ok(refs)
}

/// `git log -z --format=...` emits six NUL-separated fields plus decorations,
/// then a NUL record terminator. `%D` uses comma-space between decorations;
/// Git disallows spaces in ref names. Commit fields are not whitespace-tokenized.
pub fn parse_history(bytes: &[u8]) -> Result<Vec<CommitInfo>, AppError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) {
        return Err(malformed());
    }
    let fields: Vec<&[u8]> = bytes[..bytes.len() - 1].split(|byte| *byte == 0).collect();
    if fields.len() % 7 != 0 {
        return Err(malformed());
    }
    let mut commits = Vec::with_capacity(fields.len() / 7);
    for row in fields.chunks_exact(7) {
        let parent_hashes = if row[1].is_empty() {
            Vec::new()
        } else {
            row[1]
                .split(|byte| *byte == b' ')
                .map(hash)
                .collect::<Result<Vec<_>, _>>()?
        };
        commits.push(CommitInfo {
            hash: hash(row[0])?,
            parent_hashes,
            author_name: text(row[2])?,
            author_email: text(row[3])?,
            timestamp: text(row[4])?,
            subject: text(row[5])?,
            refs: parse_refs(row[6])?,
        });
    }
    Ok(commits)
}

fn decode_cursor(cursor: Option<&str>) -> Result<(usize, Option<&str>, GraphState), AppError> {
    let Some(cursor) = cursor else {
        return Ok((0, None, GraphState::default()));
    };
    if cursor.len() > 300_000 {
        return Err(AppError::new("invalid_cursor", "Reload commit history."));
    }
    let mut fields = cursor.splitn(3, ':');
    let offset = fields.next().unwrap_or_default();
    let predecessor = fields
        .next()
        .ok_or_else(|| AppError::new("invalid_cursor", "Reload commit history."))?;
    let graph = fields
        .next()
        .ok_or_else(|| AppError::new("invalid_cursor", "Reload commit history."))?;
    let offset = offset
        .parse::<usize>()
        .map_err(|_| AppError::new("invalid_cursor", "Reload commit history."))?;
    if offset == 0 || offset > 1_000_000 || hash(predecessor.as_bytes()).is_err() {
        return Err(AppError::new("invalid_cursor", "Reload commit history."));
    }
    let graph = GraphState::decode(graph)
        .map_err(|_| AppError::new("invalid_cursor", "Reload commit history."))?;
    Ok((offset, Some(predecessor), graph))
}

pub async fn load_history(
    git: &GitRunner,
    repository: &Path,
    cursor: Option<&str>,
) -> Result<(CommitHistoryPage, HistoryTiming), AppError> {
    let started = Instant::now();
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let (offset, predecessor, mut graph_state) = decode_cursor(cursor)?;
    let skip = if predecessor.is_some() { offset - 1 } else { 0 };
    let max = PAGE_SIZE + usize::from(predecessor.is_some()) + 1;
    let skip_arg = format!("--skip={skip}");
    let max_arg = format!("--max-count={max}");
    let format_arg = format!("--format={FORMAT}");
    let output = git
        .run(
            repository,
            &[
                "log",
                "--all",
                "--topo-order",
                "--date-order",
                "--decorate=short",
                "-z",
                &format_arg,
                &skip_arg,
                &max_arg,
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let mut commits = parse_history(&output.stdout)?;
    let parse = parsing.elapsed();
    if let Some(predecessor) = predecessor {
        if commits.first().map(|commit| commit.hash.as_str()) != Some(predecessor) {
            return Err(AppError::new(
                "history_changed",
                "Commit history changed. Reload it to continue.",
            ));
        }
        commits.remove(0);
    }
    let has_more = commits.len() > PAGE_SIZE;
    commits.truncate(PAGE_SIZE);
    let graph_started = Instant::now();
    let graph_rows = layout_page(&mut graph_state, &commits).map_err(|error| {
        AppError::new(
            "invalid_graph_topology",
            format!("Commit graph could not be laid out: {error:?}."),
        )
    })?;
    let graph = graph_started.elapsed();
    let next_cursor = if has_more {
        commits.last().map(|commit| {
            format!(
                "{}:{}:{}",
                offset + commits.len(),
                commit.hash,
                graph_state.encode()
            )
        })
    } else {
        None
    };
    let timing = HistoryTiming {
        git: output.duration,
        parse,
        graph,
        total: started.elapsed(),
        output_bytes: output.stdout.len(),
    };
    eprintln!(
        "Git.Log git_ms={} parse_us={} graph_us={} total_ms={} rows={} bytes={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.graph.as_micros(),
        timing.total.as_millis(),
        commits.len(),
        timing.output_bytes
    );
    Ok((
        CommitHistoryPage {
            commits,
            graph_rows,
            next_cursor,
            has_more,
        },
        timing,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str, parents: &str, author: &str, subject: &str, refs: &str) -> Vec<u8> {
        format!("{hash}\0{parents}\0{author}\0a@example.com\02026-01-01T00:00:00+00:00\0{subject}\0{refs}\0").into_bytes()
    }

    #[test]
    fn root_merge_unicode_refs_and_unusual_subject() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let c = "c".repeat(40);
        let mut bytes = record(
            &a,
            "",
            "林明",
            "Root: tabs\tand commas, here",
            "HEAD -> main, tag: v1",
        );
        bytes.extend(record(&c, &format!("{a} {b}"), "Zoë", "Merge 🌱", ""));
        let commits = parse_history(&bytes).unwrap();
        assert_eq!(commits[0].parent_hashes, Vec::<String>::new());
        assert_eq!(commits[0].refs, vec!["HEAD -> main", "tag: v1"]);
        assert_eq!(commits[0].subject, "Root: tabs\tand commas, here");
        assert_eq!(commits[1].parent_hashes, vec![a, b]);
        assert_eq!(commits[1].author_name, "Zoë");
        assert_eq!(commits[1].subject, "Merge 🌱");
    }

    #[test]
    fn malformed_record_is_rejected() {
        assert_eq!(
            parse_history(b"partial").unwrap_err().code,
            "invalid_git_output"
        );
        assert!(parse_history(b"").unwrap().is_empty());
    }

    #[test]
    fn graph_cursor_is_opaque_and_validated() {
        let predecessor = "a".repeat(40);
        let cursor = format!("100:{predecessor}:{predecessor}");
        let (offset, hash, state) = decode_cursor(Some(&cursor)).unwrap();
        assert_eq!(offset, 100);
        assert_eq!(hash, Some(predecessor.as_str()));
        assert_eq!(state.active_count(), 1);
        assert_eq!(
            decode_cursor(Some(&format!("100:{predecessor}:-")))
                .unwrap_err()
                .code,
            "invalid_cursor"
        );
    }
}
