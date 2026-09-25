use crate::{
    error::{git_failure, AppError},
    git::{GitOutput, GitRunner},
    status::{ChangeKind, FileChange},
    working_tree::valid_relative_path,
};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIFF_LINES: usize = 20_000;
const MAX_FILE_LIST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
    NoNewline,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line_number: Option<u32>,
    pub new_line_number: Option<u32>,
    pub content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub change: FileChange,
    pub is_binary: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug)]
pub struct DiffTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
    pub output_bytes: usize,
}

fn malformed() -> AppError {
    AppError::new("invalid_git_output", "Git returned malformed diff data.")
}
fn too_large() -> AppError {
    AppError::new(
        "diff_too_large",
        "This diff is too large to display safely.",
    )
}

fn range(text: &str) -> Result<(u32, u32), AppError> {
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    Ok((
        start.parse().map_err(|_| malformed())?,
        count.parse().map_err(|_| malformed())?,
    ))
}

fn hunk_header(line: &str) -> Result<DiffHunk, AppError> {
    let range_end = line[3..].find(" @@").ok_or_else(malformed)? + 3;
    let (old, new) = line[3..range_end].split_once(" +").ok_or_else(malformed)?;
    let (old_start, old_count) = range(old.strip_prefix('-').ok_or_else(malformed)?)?;
    let (new_start, new_count) = range(new)?;
    Ok(DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        header: line.to_owned(),
        lines: Vec::new(),
    })
}

/// Parses one targeted Git patch. Paths and change kind come from the structured
/// status/name-status selection, avoiding the quoted, human-oriented patch path
/// headers. Multiple file patches are rejected rather than silently mixed.
pub fn parse_patch(bytes: &[u8], change: FileChange) -> Result<DiffFile, AppError> {
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(too_large());
    }
    let patch = std::str::from_utf8(bytes)
        .map_err(|_| AppError::new("invalid_diff_encoding", "This text diff is not UTF-8."))?;
    let mut file = DiffFile {
        change,
        is_binary: false,
        hunks: Vec::new(),
    };
    let mut headers = 0;
    let mut old_next = 0u32;
    let mut new_next = 0u32;
    let mut line_count = 0usize;
    for line in patch.split_terminator('\n') {
        if line.starts_with("diff --git ") {
            headers += 1;
            if headers > 1 {
                return Err(malformed());
            }
            continue;
        }
        if headers == 0 {
            return Err(malformed());
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.is_binary = true;
            continue;
        }
        if file.is_binary {
            continue;
        }
        if line.starts_with("@@ ") {
            let hunk = hunk_header(line)?;
            old_next = hunk.old_start;
            new_next = hunk.new_start;
            file.hunks.push(hunk);
            continue;
        }
        if line == "\\ No newline at end of file" {
            let hunk = file.hunks.last_mut().ok_or_else(malformed)?;
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::NoNewline,
                old_line_number: None,
                new_line_number: None,
                content: String::new(),
            });
            continue;
        }
        if let Some(hunk) = file.hunks.last_mut() {
            let (kind, old_number, new_number, content) = match line.as_bytes().first() {
                Some(b' ') => {
                    let old = old_next;
                    let new = new_next;
                    old_next += 1;
                    new_next += 1;
                    (DiffLineKind::Context, Some(old), Some(new), &line[1..])
                }
                Some(b'+') => {
                    let new = new_next;
                    new_next += 1;
                    (DiffLineKind::Addition, None, Some(new), &line[1..])
                }
                Some(b'-') => {
                    let old = old_next;
                    old_next += 1;
                    (DiffLineKind::Deletion, Some(old), None, &line[1..])
                }
                _ => return Err(malformed()),
            };
            line_count += 1;
            if line_count > MAX_DIFF_LINES {
                return Err(too_large());
            }
            hunk.lines.push(DiffLine {
                kind,
                old_line_number: old_number,
                new_line_number: new_number,
                content: content.to_owned(),
            });
        } else if !line.starts_with("index ")
            && !line.starts_with("--- ")
            && !line.starts_with("+++ ")
            && !line.starts_with("new file mode ")
            && !line.starts_with("deleted file mode ")
            && !line.starts_with("old mode ")
            && !line.starts_with("new mode ")
            && !line.starts_with("similarity index ")
            && !line.starts_with("dissimilarity index ")
            && !line.starts_with("rename from ")
            && !line.starts_with("rename to ")
            && !line.starts_with("copy from ")
            && !line.starts_with("copy to ")
        {
            return Err(malformed());
        }
    }
    if !bytes.is_empty() && headers == 0 {
        return Err(malformed());
    }
    for hunk in &file.hunks {
        let old_lines = hunk
            .lines
            .iter()
            .filter(|line| matches!(line.kind, DiffLineKind::Context | DiffLineKind::Deletion))
            .count();
        let new_lines = hunk
            .lines
            .iter()
            .filter(|line| matches!(line.kind, DiffLineKind::Context | DiffLineKind::Addition))
            .count();
        if old_lines != hunk.old_count as usize || new_lines != hunk.new_count as usize {
            return Err(malformed());
        }
    }
    Ok(file)
}

pub fn validate_commit(hash: &str) -> Result<(), AppError> {
    if (hash.len() != 40 && hash.len() != 64) || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::new("invalid_commit", "Choose a valid commit."));
    }
    Ok(())
}

pub fn parse_name_status(bytes: &[u8]) -> Result<Vec<FileChange>, AppError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) {
        return Err(malformed());
    }
    let mut tokens = bytes[..bytes.len() - 1].split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(code) = tokens.next() {
        let kind = match code.first() {
            Some(b'A') => ChangeKind::Added,
            Some(b'M') => ChangeKind::Modified,
            Some(b'D') => ChangeKind::Deleted,
            Some(b'R') => ChangeKind::Renamed,
            Some(b'C') => ChangeKind::Copied,
            Some(b'T') => ChangeKind::TypeChanged,
            _ => return Err(malformed()),
        };
        let path = tokens.next().ok_or_else(malformed)?;
        let path = std::str::from_utf8(path).map_err(|_| malformed())?;
        if path.is_empty() {
            return Err(malformed());
        }
        let (path, old_path) = if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
            let new = tokens.next().ok_or_else(malformed)?;
            let new = std::str::from_utf8(new).map_err(|_| malformed())?;
            if new.is_empty() {
                return Err(malformed());
            }
            (new.to_owned(), Some(path.to_owned()))
        } else {
            (path.to_owned(), None)
        };
        files.push(FileChange {
            path,
            old_path,
            kind,
        });
        if files.len() > 10_000 {
            return Err(too_large());
        }
    }
    Ok(files)
}

fn paths(change: &FileChange) -> Result<Vec<&str>, AppError> {
    valid_relative_path(&change.path)?;
    let mut values = vec![change.path.as_str()];
    if let Some(old) = change.old_path.as_deref() {
        valid_relative_path(old)?;
        values.push(old);
    }
    Ok(values)
}

async fn parse_output(
    output: GitOutput,
    change: FileChange,
    started: Instant,
) -> Result<(DiffFile, DiffTiming), AppError> {
    let parsing = Instant::now();
    let file = parse_patch(&output.stdout, change)?;
    let timing = DiffTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
        output_bytes: output.stdout.len(),
    };
    eprintln!(
        "Git.Diff git_ms={} parse_us={} total_ms={} bytes={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis(),
        timing.output_bytes
    );
    Ok((file, timing))
}

pub async fn load_working_diff(
    git: &GitRunner,
    repository: &Path,
    staged: bool,
    untracked: bool,
    change: FileChange,
) -> Result<(DiffFile, DiffTiming), AppError> {
    let started = Instant::now();
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let selected_paths = paths(&change)?;
    let mut args = vec![
        "--literal-pathspecs",
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--find-renames",
        "--unified=3",
    ];
    if untracked {
        args.push("--no-index");
    }
    if staged {
        args.push("--cached");
    }
    args.push("--");
    if untracked {
        args.push("/dev/null");
    }
    args.extend(selected_paths);
    let output = git
        .run_limited(repository, &args, MAX_PATCH_BYTES)
        .await
        .map_err(AppError::from)?;
    if !output.success() && !(untracked && output.exit_code == Some(1)) {
        return Err(git_failure(&output));
    }
    parse_output(output, change, started).await
}

pub async fn load_commit_files(
    git: &GitRunner,
    repository: &Path,
    commit: &str,
) -> Result<Vec<FileChange>, AppError> {
    validate_commit(commit)?;
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let output = git
        .run_limited(
            repository,
            &[
                "show",
                "--format=",
                "--root",
                "--diff-merges=first-parent",
                "--name-status",
                "-z",
                "--find-renames",
                commit,
            ],
            MAX_FILE_LIST_BYTES,
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    parse_name_status(&output.stdout)
}

pub async fn load_commit_diff(
    git: &GitRunner,
    repository: &Path,
    commit: &str,
    change: FileChange,
) -> Result<(DiffFile, DiffTiming), AppError> {
    let started = Instant::now();
    validate_commit(commit)?;
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let selected_paths = paths(&change)?;
    let mut args = vec![
        "--literal-pathspecs",
        "show",
        "--format=",
        "--root",
        "--diff-merges=first-parent",
        "--patch",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--find-renames",
        "--unified=3",
        commit,
        "--",
    ];
    args.extend(selected_paths);
    let output = git
        .run_limited(repository, &args, MAX_PATCH_BYTES)
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    parse_output(output, change, started).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: ChangeKind) -> FileChange {
        FileChange {
            path: "新 file.txt".into(),
            old_path: None,
            kind,
        }
    }

    #[test]
    fn parses_modified_multiple_hunks_numbers_unicode_and_no_newline() {
        let patch = b"diff --git a/file b/file\nindex 123..456 100644\n--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@ section\n old\n-before\n+after\n@@ -10 +10 @@\n-\xe6\x97\xa7\n+\xe6\x96\xb0\n\\ No newline at end of file\n";
        let file = parse_patch(patch, change(ChangeKind::Modified)).unwrap();
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(file.hunks[0].lines[0].old_line_number, Some(1));
        assert_eq!(file.hunks[0].lines[0].new_line_number, Some(1));
        assert_eq!(file.hunks[0].lines[1].old_line_number, Some(2));
        assert_eq!(file.hunks[0].lines[1].new_line_number, None);
        assert_eq!(file.hunks[0].lines[2].new_line_number, Some(2));
        assert_eq!(file.hunks[1].lines[1].content, "新");
        assert!(matches!(
            file.hunks[1].lines[2].kind,
            DiffLineKind::NoNewline
        ));
        assert_eq!(file.hunks[1].lines[2].new_line_number, None);
    }

    #[test]
    fn added_deleted_rename_binary_and_multiple_files() {
        let added = b"diff --git a/new b/new\nnew file mode 100644\n--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+hello\n";
        assert_eq!(
            parse_patch(added, change(ChangeKind::Added)).unwrap().hunks[0].lines[0]
                .new_line_number,
            Some(1)
        );
        let deleted = b"diff --git a/old b/old\ndeleted file mode 100644\n--- a/old\n+++ /dev/null\n@@ -1 +0,0 @@\n-goodbye\n";
        assert_eq!(
            parse_patch(deleted, change(ChangeKind::Deleted))
                .unwrap()
                .hunks[0]
                .lines[0]
                .old_line_number,
            Some(1)
        );
        let rename = b"diff --git a/old name b/new name\nsimilarity index 100%\nrename from old name\nrename to new name\n";
        assert!(parse_patch(rename, change(ChangeKind::Renamed))
            .unwrap()
            .hunks
            .is_empty());
        let binary = b"diff --git a/image b/image\nindex 123..456 100644\nBinary files a/image and b/image differ\n";
        assert!(
            parse_patch(binary, change(ChangeKind::Modified))
                .unwrap()
                .is_binary
        );
        let mut two = added.to_vec();
        two.extend_from_slice(deleted);
        assert_eq!(
            parse_patch(&two, change(ChangeKind::Modified))
                .unwrap_err()
                .code,
            "invalid_git_output"
        );
    }

    #[test]
    fn name_status_preserves_spaces_unicode_and_rename_order() {
        let files = parse_name_status(
            "M\0space file.txt\0R100\0old 名.txt\0new 名.txt\0A\0added.txt\0".as_bytes(),
        )
        .unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "space file.txt");
        assert_eq!(files[1].old_path.as_deref(), Some("old 名.txt"));
        assert_eq!(files[1].path, "new 名.txt");
        assert_eq!(files[2].kind, ChangeKind::Added);
    }
}
