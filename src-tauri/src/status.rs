use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflicted,
    Untracked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RepositoryStatus {
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub untracked: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

fn invalid_output() -> AppError {
    AppError::new("invalid_git_output", "Git returned malformed status data.")
}

fn path_field<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a str, AppError> {
    let remainder = &bytes[*cursor..];
    let end = remainder
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(invalid_output)?;
    let path = std::str::from_utf8(&remainder[..end]).map_err(|_| {
        AppError::new(
            "invalid_path_encoding",
            "A Git path is not valid UTF-8 and cannot be shown safely.",
        )
    })?;
    if path.is_empty() {
        return Err(invalid_output());
    }
    *cursor += end + 1;
    Ok(path)
}

fn kind(code: u8) -> Result<Option<ChangeKind>, AppError> {
    Ok(match code {
        b' ' => None,
        b'A' => Some(ChangeKind::Added),
        b'M' => Some(ChangeKind::Modified),
        b'D' => Some(ChangeKind::Deleted),
        b'R' => Some(ChangeKind::Renamed),
        b'C' => Some(ChangeKind::Copied),
        b'T' => Some(ChangeKind::TypeChanged),
        b'U' => Some(ChangeKind::Conflicted),
        _ => return Err(invalid_output()),
    })
}

fn is_conflict(x: u8, y: u8) -> bool {
    matches!(
        (x, y),
        (b'D', b'D')
            | (b'A', b'U')
            | (b'U', b'D')
            | (b'U', b'A')
            | (b'D', b'U')
            | (b'A', b'A')
            | (b'U', b'U')
    )
}

/// Parses `git status --porcelain=v1 -z` without splitting on whitespace.
/// For renames and copies, Git emits the destination path before the source path.
pub fn parse_status(bytes: &[u8]) -> Result<RepositoryStatus, AppError> {
    let mut status = RepositoryStatus::default();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes.len() - cursor < 4 || bytes[cursor + 2] != b' ' {
            return Err(invalid_output());
        }
        let x = bytes[cursor];
        let y = bytes[cursor + 1];
        cursor += 3;
        let path = path_field(bytes, &mut cursor)?.to_owned();
        if (x, y) == (b'?', b'?') {
            status.untracked.push(FileChange {
                path,
                old_path: None,
                kind: ChangeKind::Untracked,
            });
            continue;
        }
        if x == b'?' || y == b'?' || x == b'!' || y == b'!' {
            return Err(invalid_output());
        }
        let old_path = if x == b'R' || x == b'C' || y == b'R' || y == b'C' {
            Some(path_field(bytes, &mut cursor)?.to_owned())
        } else {
            None
        };
        if is_conflict(x, y) {
            status.conflicted.push(FileChange {
                path,
                old_path,
                kind: ChangeKind::Conflicted,
            });
            continue;
        }
        if let Some(change_kind) = kind(x)? {
            status.staged.push(FileChange {
                path: path.clone(),
                old_path: old_path.clone(),
                kind: change_kind,
            });
        }
        if let Some(change_kind) = kind(y)? {
            status.unstaged.push(FileChange {
                path,
                old_path,
                kind: change_kind,
            });
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_staged_unstaged_untracked_and_combined_changes() {
        let status = parse_status(b"M  staged.txt\0 M loose file.txt\0AM both.txt\0A  added.txt\0 D removed.txt\0?? \xe6\xb8\xac\xe8\xa9\xa6.txt\0").unwrap();
        assert_eq!(status.staged.len(), 3);
        assert_eq!(status.staged[0].kind, ChangeKind::Modified);
        assert_eq!(status.staged[1].path, "both.txt");
        assert_eq!(status.staged[2].kind, ChangeKind::Added);
        assert_eq!(status.unstaged.len(), 3);
        assert_eq!(status.unstaged[0].path, "loose file.txt");
        assert_eq!(status.unstaged[1].kind, ChangeKind::Modified);
        assert_eq!(status.unstaged[2].kind, ChangeKind::Deleted);
        assert_eq!(status.untracked[0].path, "測試.txt");
    }

    #[test]
    fn parses_renames_copies_and_conflicts() {
        let status = parse_status(
            b"R  new name.txt\0old name.txt\0 C copied.txt\0source.txt\0UU conflict.txt\0",
        )
        .unwrap();
        assert_eq!(status.staged[0].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(status.staged[0].kind, ChangeKind::Renamed);
        assert_eq!(status.unstaged[0].old_path.as_deref(), Some("source.txt"));
        assert_eq!(status.unstaged[0].kind, ChangeKind::Copied);
        assert_eq!(status.conflicted[0].kind, ChangeKind::Conflicted);
    }

    #[test]
    fn rejects_truncated_or_invalid_output() {
        assert!(parse_status(b"M  missing-terminator").is_err());
        assert!(parse_status(b"R  destination\0").is_err());
        assert!(parse_status(b"?? \xff\0").is_err());
        assert!(parse_status(b"Q  unknown\0").is_err());
    }
}
