use crate::{
    error::{git_failure, AppError},
    git::{GitRunError, GitRunner},
    status::RepositoryStatus,
    working_tree::valid_relative_path,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_CONTENT: usize = 256 * 1024;
const MAX_INDEX_OUTPUT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryOperationState {
    None,
    Merge {
        #[serde(rename = "mergeHeads")]
        merge_heads: Vec<String>,
        message: String,
    },
    Rebase {
        #[serde(rename = "currentCommit")]
        current_commit: Option<String>,
        step: Option<u32>,
        total: Option<u32>,
    },
    CherryPick {
        commit: String,
    },
    Revert {
        commit: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    BothModified,
    BothAdded,
    DeletedByUs,
    DeletedByThem,
    AddedByUs,
    AddedByThem,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictStage {
    pub object_hash: String,
    pub mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    pub kind: ConflictKind,
    pub base: Option<ConflictStage>,
    pub ours: Option<ConflictStage>,
    pub theirs: Option<ConflictStage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryState {
    pub status: RepositoryStatus,
    pub operation: RepositoryOperationState,
    pub conflicts: Vec<ConflictFile>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPart {
    pub text: Option<String>,
    pub is_binary: bool,
    pub oversized: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictContent {
    pub path: String,
    pub base: Option<ContentPart>,
    pub ours: Option<ContentPart>,
    pub theirs: Option<ContentPart>,
    pub working: Option<ContentPart>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    Ours,
    Theirs,
}

fn malformed() -> AppError {
    AppError::new(
        "invalid_git_output",
        "Git returned malformed unmerged index data.",
    )
}

fn valid_hash(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn conflict_kind(base: bool, ours: bool, theirs: bool) -> ConflictKind {
    match (base, ours, theirs) {
        (true, true, true) => ConflictKind::BothModified,
        (false, true, true) => ConflictKind::BothAdded,
        (true, false, true) => ConflictKind::DeletedByUs,
        (true, true, false) => ConflictKind::DeletedByThem,
        (false, true, false) => ConflictKind::AddedByUs,
        (false, false, true) => ConflictKind::AddedByThem,
        _ => ConflictKind::Other,
    }
}

/// `git ls-files -u -z`: mode SP object SP stage TAB path NUL.
/// The path is never whitespace-split. BTreeMap makes display order stable.
pub fn parse_unmerged(bytes: &[u8]) -> Result<Vec<ConflictFile>, AppError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(malformed());
    }
    let mut files: BTreeMap<String, [Option<ConflictStage>; 3]> = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(malformed)?;
        let header = std::str::from_utf8(&record[..tab]).map_err(|_| malformed())?;
        let mut pieces = header.split(' ');
        let mode = pieces.next().ok_or_else(malformed)?;
        let hash = pieces.next().ok_or_else(malformed)?;
        let stage: usize = pieces
            .next()
            .ok_or_else(malformed)?
            .parse()
            .map_err(|_| malformed())?;
        if pieces.next().is_some()
            || !(1..=3).contains(&stage)
            || mode.is_empty()
            || !mode.bytes().all(|byte| byte.is_ascii_digit())
            || !valid_hash(hash)
        {
            return Err(malformed());
        }
        let path = std::str::from_utf8(&record[tab + 1..]).map_err(|_| {
            AppError::new(
                "invalid_path_encoding",
                "A conflicted path is not valid UTF-8.",
            )
        })?;
        valid_relative_path(path)?;
        let slots = files.entry(path.to_owned()).or_default();
        if slots[stage - 1].is_some() {
            return Err(malformed());
        }
        slots[stage - 1] = Some(ConflictStage {
            object_hash: hash.to_owned(),
            mode: mode.to_owned(),
        });
    }
    Ok(files
        .into_iter()
        .map(|(path, [base, ours, theirs])| ConflictFile {
            path,
            kind: conflict_kind(base.is_some(), ours.is_some(), theirs.is_some()),
            base,
            ours,
            theirs,
        })
        .collect())
}

fn metadata_path(repository: &Path, output: &str) -> PathBuf {
    let path = Path::new(output);
    if path.is_absolute() {
        path.to_owned()
    } else {
        repository.join(path)
    }
}

pub async fn operation_state(
    git: &GitRunner,
    repository: &Path,
) -> Result<RepositoryOperationState, AppError> {
    let output = git
        .run(
            repository,
            &[
                "rev-parse",
                "--git-path",
                "rebase-merge",
                "--git-path",
                "rebase-apply",
                "--git-path",
                "MERGE_HEAD",
                "--git-path",
                "CHERRY_PICK_HEAD",
                "--git-path",
                "REVERT_HEAD",
                "--git-path",
                "REBASE_HEAD",
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let paths: Vec<_> = output
        .stdout_text()
        .lines()
        .map(|line| metadata_path(repository, line))
        .collect();
    if paths.len() != 6 {
        return Err(malformed());
    }
    for directory in [&paths[0], &paths[1]] {
        if directory.is_dir() {
            if directory == &paths[1] && !directory.join("rebasing").is_file() {
                return Err(AppError::new(
                    "unsupported_operation",
                    "A Git am session is in progress; Woo cannot continue it as a rebase.",
                ));
            }
            let read_number = |name: &str| -> Option<u32> {
                let mut bytes = Vec::new();
                File::open(directory.join(name))
                    .ok()?
                    .take(32)
                    .read_to_end(&mut bytes)
                    .ok()?;
                std::str::from_utf8(&bytes).ok()?.trim().parse().ok()
            };
            let current_commit = read_marker_hash(&paths[5])?;
            return Ok(RepositoryOperationState::Rebase {
                current_commit,
                step: read_number(if directory == &paths[0] {
                    "msgnum"
                } else {
                    "next"
                }),
                total: read_number(if directory == &paths[0] {
                    "end"
                } else {
                    "last"
                }),
            });
        }
    }
    let location = &paths[2];
    let file = match File::open(location) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(commit) = read_marker_hash(&paths[3])? {
                return Ok(RepositoryOperationState::CherryPick { commit });
            }
            if let Some(commit) = read_marker_hash(&paths[4])? {
                return Ok(RepositoryOperationState::Revert { commit });
            }
            return Ok(RepositoryOperationState::None);
        }
        Err(error) => {
            return Err(AppError::new(
                "merge_state_unavailable",
                format!("Could not read merge state: {error}"),
            ))
        }
    };
    let mut head = Vec::new();
    file.take(1025).read_to_end(&mut head).map_err(|error| {
        AppError::new(
            "merge_state_unavailable",
            format!("Could not read merge state: {error}"),
        )
    })?;
    if head.len() > 1024 {
        return Err(malformed());
    }
    let head = std::str::from_utf8(&head).map_err(|_| malformed())?;
    let merge_heads: Vec<_> = head.lines().map(str::to_owned).collect();
    if merge_heads.is_empty() || merge_heads.iter().any(|hash| !valid_hash(hash)) {
        return Err(malformed());
    }
    let message_path = location.with_file_name("MERGE_MSG");
    let message = match fs::metadata(&message_path) {
        Ok(metadata) if metadata.len() > MAX_CONTENT as u64 => String::new(),
        Ok(_) => {
            let mut bytes = Vec::new();
            File::open(&message_path)
                .and_then(|file| file.take(MAX_CONTENT as u64 + 1).read_to_end(&mut bytes))
                .map_err(|error| {
                    AppError::new(
                        "merge_state_unavailable",
                        format!("Could not read the merge message: {error}"),
                    )
                })?;
            if bytes.len() > MAX_CONTENT {
                String::new()
            } else {
                String::from_utf8(bytes).map_err(|_| {
                    AppError::new(
                        "merge_state_unavailable",
                        "The merge message is not valid UTF-8.",
                    )
                })?
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(AppError::new(
                "merge_state_unavailable",
                format!("Could not inspect the merge message: {error}"),
            ))
        }
    };
    Ok(RepositoryOperationState::Merge {
        merge_heads,
        message,
    })
}

fn read_marker_hash(path: &Path) -> Result<Option<String>, AppError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::new(
                "operation_state_unavailable",
                format!("Could not read Git operation state: {error}"),
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes).map_err(|error| {
        AppError::new(
            "operation_state_unavailable",
            format!("Could not read Git operation state: {error}"),
        )
    })?;
    if bytes.len() > 128 {
        return Err(malformed());
    }
    let hash = std::str::from_utf8(&bytes).map_err(|_| malformed())?.trim();
    if !valid_hash(hash) {
        return Err(malformed());
    }
    Ok(Some(hash.to_owned()))
}

pub async fn list_conflicts(
    git: &GitRunner,
    repository: &Path,
) -> Result<Vec<ConflictFile>, AppError> {
    let output = git
        .run_limited(repository, &["ls-files", "-u", "-z"], MAX_INDEX_OUTPUT)
        .await
        .map_err(|error| match error {
            GitRunError::OutputLimit => AppError::new(
                "too_many_conflicts",
                "The unmerged index is too large to display safely.",
            ),
            other => AppError::from(other),
        })?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    parse_unmerged(&output.stdout)
}

pub async fn load_state(
    git: &GitRunner,
    repository: &Path,
    status: RepositoryStatus,
) -> Result<RepositoryState, AppError> {
    let operation = operation_state(git, repository).await?;
    let conflicts = if status.conflicted.is_empty() {
        Vec::new()
    } else {
        list_conflicts(git, repository).await?
    };
    Ok(RepositoryState {
        status,
        operation,
        conflicts,
    })
}

pub fn selected<'a>(
    conflicts: &'a [ConflictFile],
    path: &str,
) -> Result<&'a ConflictFile, AppError> {
    valid_relative_path(path)?;
    conflicts
        .iter()
        .find(|file| file.path == path)
        .ok_or_else(|| {
            AppError::new(
                "conflict_missing",
                "This file is no longer conflicted. Refresh repository state.",
            )
        })
}

fn content_part(bytes: Vec<u8>, special_mode: bool) -> ContentPart {
    if special_mode || bytes.contains(&0) {
        return ContentPart {
            text: None,
            is_binary: true,
            oversized: false,
        };
    }
    match String::from_utf8(bytes) {
        Ok(text) => ContentPart {
            text: Some(text),
            is_binary: false,
            oversized: false,
        },
        Err(_) => ContentPart {
            text: None,
            is_binary: true,
            oversized: false,
        },
    }
}

async fn blob_content(
    git: &GitRunner,
    repository: &Path,
    stage: Option<&ConflictStage>,
) -> Result<Option<ContentPart>, AppError> {
    let Some(stage) = stage else {
        return Ok(None);
    };
    if stage.mode != "100644" && stage.mode != "100755" {
        return Ok(Some(ContentPart {
            text: None,
            is_binary: true,
            oversized: false,
        }));
    }
    let output = git
        .run_limited(
            repository,
            &["cat-file", "blob", &stage.object_hash],
            MAX_CONTENT,
        )
        .await;
    let output = match output {
        Ok(value) => value,
        Err(GitRunError::OutputLimit) => {
            return Ok(Some(ContentPart {
                text: None,
                is_binary: false,
                oversized: true,
            }))
        }
        Err(error) => return Err(AppError::from(error)),
    };
    if !output.success() {
        return Err(git_failure(&output));
    }
    Ok(Some(content_part(output.stdout, false)))
}

fn safe_working_path(repository: &Path, relative: &str) -> Result<PathBuf, AppError> {
    valid_relative_path(relative)?;
    let root = fs::canonicalize(repository).map_err(|error| {
        AppError::new(
            "repository_missing",
            format!("Could not access the repository: {error}"),
        )
    })?;
    let mut candidate = repository.to_owned();
    let components: Vec<_> = Path::new(relative).components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        candidate.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            AppError::new(
                "invalid_path",
                format!("Could not inspect a conflict parent directory: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(AppError::new(
                "invalid_path",
                "The conflict path passes through a link or non-directory.",
            ));
        }
    }
    candidate.push(
        components
            .last()
            .expect("validated nonempty path")
            .as_os_str(),
    );
    let parent = candidate
        .parent()
        .ok_or_else(|| AppError::new("invalid_path", "Invalid conflict path."))?;
    let actual_parent = fs::canonicalize(parent).map_err(|error| {
        AppError::new(
            "invalid_path",
            format!("Could not access the conflict parent directory: {error}"),
        )
    })?;
    if !actual_parent.starts_with(root) {
        return Err(AppError::new(
            "invalid_path",
            "The conflict path leaves the repository.",
        ));
    }
    Ok(candidate)
}

fn working_content(repository: &Path, relative: &str) -> Result<Option<ContentPart>, AppError> {
    let candidate = safe_working_path(repository, relative)?;
    let metadata = match fs::symlink_metadata(&candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::new(
                "conflict_unavailable",
                format!("Could not inspect the conflicted file: {error}"),
            ))
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(Some(ContentPart {
            text: None,
            is_binary: true,
            oversized: false,
        }));
    }
    if metadata.len() > MAX_CONTENT as u64 {
        return Ok(Some(ContentPart {
            text: None,
            is_binary: false,
            oversized: true,
        }));
    }
    let mut bytes = Vec::new();
    File::open(&candidate)
        .and_then(|file| file.take(MAX_CONTENT as u64 + 1).read_to_end(&mut bytes))
        .map_err(|error| {
            AppError::new(
                "conflict_unavailable",
                format!("Could not read the conflicted file: {error}"),
            )
        })?;
    if bytes.len() > MAX_CONTENT {
        return Ok(Some(ContentPart {
            text: None,
            is_binary: false,
            oversized: true,
        }));
    }
    Ok(Some(content_part(bytes, false)))
}

pub async fn load_content(
    git: &GitRunner,
    repository: &Path,
    file: &ConflictFile,
) -> Result<ConflictContent, AppError> {
    Ok(ConflictContent {
        path: file.path.clone(),
        base: blob_content(git, repository, file.base.as_ref()).await?,
        ours: blob_content(git, repository, file.ours.as_ref()).await?,
        theirs: blob_content(git, repository, file.theirs.as_ref()).await?,
        working: working_content(repository, &file.path)?,
    })
}

pub fn save_text(
    repository: &Path,
    path: &str,
    expected: Option<&str>,
    text: &str,
) -> Result<(), AppError> {
    if text.len() > MAX_CONTENT || text.contains('\0') {
        return Err(AppError::new(
            "conflict_too_large",
            "The resolved text is too large or contains binary data.",
        ));
    }
    let target = safe_working_path(repository, path)?;
    let current = working_content(repository, path)?;
    if current
        .as_ref()
        .is_some_and(|part| part.is_binary || part.oversized)
    {
        return Err(AppError::new(
            "conflict_not_text",
            "This working file cannot be edited as text.",
        ));
    }
    if current.as_ref().and_then(|part| part.text.as_deref()) != expected {
        return Err(AppError::new(
            "conflict_changed",
            "The file changed since it was loaded. Reload it before saving.",
        ));
    }
    let parent = target.parent().expect("validated path has parent");
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        AppError::new(
            "resolution_write_failed",
            format!("Could not prepare the resolved file: {error}"),
        )
    })?;
    if let Ok(metadata) = fs::metadata(&target) {
        temp.as_file()
            .set_permissions(metadata.permissions())
            .map_err(|error| {
                AppError::new(
                    "resolution_write_failed",
                    format!("Could not preserve file permissions: {error}"),
                )
            })?;
    }
    temp.write_all(text.as_bytes())
        .and_then(|_| temp.flush())
        .map_err(|error| {
            AppError::new(
                "resolution_write_failed",
                format!("Could not write the resolved file: {error}"),
            )
        })?;
    temp.persist(&target).map_err(|error| {
        AppError::new(
            "resolution_write_failed",
            format!("Could not replace the conflicted file: {}", error.error),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stages_missing_sides_unicode_and_order() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let c = "c".repeat(40);
        let bytes = format!(
            "100644 {a} 1\tz.txt\0\
            100644 {b} 2\tz.txt\0\
            100644 {c} 3\tz.txt\0\
            100644 {b} 2\t測試 space.txt\0\
            100644 {c} 3\t測試 space.txt\0\
            100644 {a} 1\tdeleted.txt\0\
            100644 {b} 2\tdeleted.txt\0"
        );
        let files = parse_unmerged(bytes.as_bytes()).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "deleted.txt");
        assert_eq!(files[0].kind, ConflictKind::DeletedByThem);
        assert!(files[0].theirs.is_none());
        assert_eq!(files[1].kind, ConflictKind::BothModified);
        assert_eq!(files[2].path, "測試 space.txt");
        assert_eq!(files[2].kind, ConflictKind::BothAdded);
    }

    #[test]
    fn rejects_malformed_and_duplicate_stages() {
        let hash = "a".repeat(40);
        assert!(parse_unmerged(b"bad").is_err());
        assert!(parse_unmerged(format!("100644 {hash} 0\tx\0").as_bytes()).is_err());
        assert!(parse_unmerged(format!("100644 {hash} 2\tx\0").repeat(2).as_bytes()).is_err());
        assert!(parse_unmerged(format!("100644 {hash} 2\t../outside\0").as_bytes()).is_err());
        assert!(parse_unmerged(
            format!("100644 {hash} 2\tx\0")
                .trim_end_matches('\0')
                .as_bytes()
        )
        .is_err());
    }

    #[test]
    fn distinguishes_binary_and_text() {
        assert_eq!(
            content_part("日本語\n".as_bytes().to_vec(), false)
                .text
                .as_deref(),
            Some("日本語\n")
        );
        assert!(content_part(b"a\0b".to_vec(), false).is_binary);
        assert!(content_part(b"symlink".to_vec(), true).is_binary);
    }

    #[test]
    fn operation_serializes_for_frontend_contract() {
        let value = serde_json::to_value(RepositoryOperationState::Merge {
            merge_heads: vec!["a".repeat(40)],
            message: "Merge feature".to_owned(),
        })
        .unwrap();
        assert_eq!(value["kind"], "merge");
        assert_eq!(value["mergeHeads"][0], "a".repeat(40));
        assert_eq!(value["message"], "Merge feature");
        let rebase = serde_json::to_value(RepositoryOperationState::Rebase {
            current_commit: Some("b".repeat(40)),
            step: Some(2),
            total: Some(3),
        })
        .unwrap();
        assert_eq!(rebase["kind"], "rebase");
        assert_eq!(rebase["currentCommit"], "b".repeat(40));
        assert_eq!(rebase["step"], 2);
        let cherry = serde_json::to_value(RepositoryOperationState::CherryPick {
            commit: "c".repeat(40),
        })
        .unwrap();
        assert_eq!(cherry["kind"], "cherry_pick");
        let revert = serde_json::to_value(RepositoryOperationState::Revert {
            commit: "d".repeat(40),
        })
        .unwrap();
        assert_eq!(revert["kind"], "revert");
    }
}
