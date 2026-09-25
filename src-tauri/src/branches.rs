use crate::{
    error::{git_failure, AppError},
    git::GitRunner,
};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub full_ref_name: String,
    pub kind: BranchKind,
    pub is_current: bool,
    pub target_hash: String,
    pub upstream: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchList {
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug)]
pub struct BranchTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
}

// Git refnames cannot contain newline or NUL. The last field filters symbolic
// remote HEAD aliases; all other fields are separated by Git's %00 format atom.
const BRANCH_FORMAT: &str = "%(refname)%00%(objectname)%00%(upstream:short)%00%(HEAD)%00%(symref)";

pub fn parse_branches(bytes: &[u8]) -> Result<BranchList, AppError> {
    let mut branches = Vec::new();
    for record in bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let fields: Vec<&[u8]> = record.split(|byte| *byte == 0).collect();
        if fields.len() != 5 {
            return Err(AppError::new(
                "invalid_git_output",
                "Git returned malformed branch information.",
            ));
        }
        let field = |index| {
            std::str::from_utf8(fields[index]).map_err(|_| {
                AppError::new(
                    "invalid_git_output",
                    "Git returned a branch name with unsupported encoding.",
                )
            })
        };
        if !fields[4].is_empty() {
            continue;
        }
        let full = field(0)?;
        let (kind, name) = if let Some(name) = full.strip_prefix("refs/heads/") {
            (BranchKind::Local, name)
        } else if let Some(name) = full.strip_prefix("refs/remotes/") {
            (BranchKind::Remote, name)
        } else {
            return Err(AppError::new(
                "invalid_git_output",
                "Git returned an unexpected branch ref.",
            ));
        };
        let hash = field(1)?;
        if hash.is_empty() {
            return Err(AppError::new(
                "invalid_git_output",
                "Git returned a branch without a target.",
            ));
        }
        let upstream = field(2)?;
        branches.push(BranchInfo {
            name: name.to_string(),
            full_ref_name: full.to_string(),
            kind,
            is_current: fields[3] == b"*",
            target_hash: hash.to_string(),
            upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        });
    }
    Ok(BranchList { branches })
}

pub async fn load_branches(
    git: &GitRunner,
    repository: &Path,
) -> Result<(BranchList, BranchTiming), AppError> {
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let started = Instant::now();
    let output = git
        .run(
            repository,
            &[
                "for-each-ref",
                "--sort=refname",
                "--format",
                BRANCH_FORMAT,
                "refs/heads",
                "refs/remotes",
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let branches = parse_branches(&output.stdout)?;
    let timing = BranchTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
    };
    eprintln!(
        "Git.Branches git_ms={} parse_us={} total_ms={} count={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis(),
        branches.branches.len()
    );
    Ok((branches, timing))
}

pub fn validate_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.as_bytes().contains(&0)
        || name.len() > 1024
    {
        return Err(AppError::new(
            "invalid_branch_name",
            "Enter a valid branch name.",
        ));
    }
    Ok(())
}

pub fn local_name(full_ref: &str) -> Result<&str, AppError> {
    let name = full_ref
        .strip_prefix("refs/heads/")
        .ok_or_else(|| AppError::new("invalid_branch", "Choose a local branch."))?;
    validate_name(name)?;
    Ok(name)
}

pub async fn rename_local(
    git: &GitRunner,
    repository: &Path,
    full_ref: &str,
    new_name: &str,
) -> Result<Duration, AppError> {
    let old_name = local_name(full_ref)?;
    validate_name(new_name)?;
    let output = git
        .run(repository, &["branch", "-m", "--", old_name, new_name])
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    Ok(output.duration)
}

pub async fn delete_local(
    git: &GitRunner,
    repository: &Path,
    full_ref: &str,
) -> Result<Duration, AppError> {
    let name = local_name(full_ref)?;
    let output = git
        .run(repository, &["branch", "-d", "--", name])
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    Ok(output.duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_local_remote_unicode_slashes_and_symbolic_alias() {
        let data = "refs/heads/main\0abc\0origin/main\0*\0\nrefs/heads/feature/測試\0abc\0\0 \0\nrefs/remotes/origin/HEAD\0abc\0\0 \0refs/remotes/origin/main\nrefs/remotes/origin/main\0abc\0\0 \0\n";
        let list = parse_branches(data.as_bytes()).unwrap();
        assert_eq!(list.branches.len(), 3);
        assert!(list.branches[0].is_current);
        assert_eq!(list.branches[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(list.branches[1].name, "feature/測試");
        assert_eq!(list.branches[2].kind, BranchKind::Remote);
    }
    #[test]
    fn detached_has_no_current_branch() {
        let list = parse_branches(b"refs/heads/main\0abc\0\0 \0\n").unwrap();
        assert!(!list.branches[0].is_current);
    }
    #[test]
    fn rejects_malformed_record() {
        assert!(parse_branches(b"refs/heads/main\n").is_err());
    }
}
