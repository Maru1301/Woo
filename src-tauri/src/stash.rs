use crate::{
    error::{git_failure, AppError},
    git::{GitOutput, GitRunner},
};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashInfo {
    pub reference: String,
    pub commit_hash: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StashList {
    pub stashes: Vec<StashInfo>,
}

#[derive(Debug)]
pub struct StashTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
}

const STASH_FORMAT: &str = "--format=%gd%x00%H%x00%gs%x00%aI%x00";

fn malformed() -> AppError {
    AppError::new(
        "invalid_git_output",
        "Git returned malformed stash information.",
    )
}

pub fn parse_stashes(bytes: &[u8]) -> Result<StashList, AppError> {
    let mut stashes = Vec::new();
    for record in bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        let mut fields: Vec<_> = record.split(|byte| *byte == 0).collect();
        if fields.last().is_some_and(|field| field.is_empty()) {
            fields.pop();
        }
        if fields.len() != 4 {
            return Err(malformed());
        }
        let field = |index| std::str::from_utf8(fields[index]).map_err(|_| malformed());
        let reference = field(0)?;
        if !reference.starts_with("stash@{") || !reference.ends_with('}') {
            return Err(malformed());
        }
        let hash = field(1)?;
        if !valid_hash(hash) {
            return Err(malformed());
        }
        stashes.push(StashInfo {
            reference: reference.to_owned(),
            commit_hash: hash.to_owned(),
            message: field(2)?.to_owned(),
            timestamp: field(3)?.to_owned(),
        });
    }
    Ok(StashList { stashes })
}

fn valid_hash(hash: &str) -> bool {
    (hash.len() == 40 || hash.len() == 64) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn validate_hash(hash: &str) -> Result<(), AppError> {
    if valid_hash(hash) {
        Ok(())
    } else {
        Err(AppError::new(
            "invalid_stash",
            "Choose a stash from the current list.",
        ))
    }
}

pub fn resolve<'a>(list: &'a StashList, hash: &str) -> Result<&'a StashInfo, AppError> {
    validate_hash(hash)?;
    let mut matches = list
        .stashes
        .iter()
        .filter(|stash| stash.commit_hash == hash);
    let first = matches.next().ok_or_else(|| {
        AppError::new(
            "stash_missing",
            "The selected stash is no longer in the stash list.",
        )
    })?;
    if matches.next().is_some() {
        return Err(AppError::new(
            "stash_ambiguous",
            "Multiple stash entries have this identity. Refresh and choose a unique stash.",
        ));
    }
    Ok(first)
}

pub async fn load_stashes(
    git: &GitRunner,
    repository: &Path,
) -> Result<(StashList, StashTiming), AppError> {
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let started = Instant::now();
    let output = git
        .run(repository, &["stash", "list", STASH_FORMAT])
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let stashes = parse_stashes(&output.stdout)?;
    let timing = StashTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
    };
    eprintln!(
        "Git.StashList git_ms={} parse_us={} total_ms={} count={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis(),
        stashes.stashes.len()
    );
    Ok((stashes, timing))
}

pub fn validate_message(message: &str) -> Result<(), AppError> {
    if message.as_bytes().contains(&0) || message.len() > 1024 * 1024 {
        return Err(AppError::new(
            "invalid_stash_message",
            "The stash message is too large or contains unsupported content.",
        ));
    }
    Ok(())
}

pub async fn create(
    git: &GitRunner,
    repository: &Path,
    message: Option<&str>,
) -> Result<GitOutput, AppError> {
    let output = if let Some(message) = message.filter(|message| !message.trim().is_empty()) {
        validate_message(message)?;
        git.run(repository, &["stash", "push", "-m", message])
            .await
            .map_err(AppError::from)?
    } else {
        git.run(repository, &["stash", "push"])
            .await
            .map_err(AppError::from)?
    };
    Ok(output)
}

pub async fn apply(git: &GitRunner, repository: &Path, hash: &str) -> Result<GitOutput, AppError> {
    validate_hash(hash)?;
    git.run(repository, &["stash", "apply", hash])
        .await
        .map_err(AppError::from)
}

pub async fn pop(
    git: &GitRunner,
    repository: &Path,
    reference: &str,
) -> Result<GitOutput, AppError> {
    git.run(repository, &["stash", "pop", reference])
        .await
        .map_err(AppError::from)
}

pub async fn drop_stash(
    git: &GitRunner,
    repository: &Path,
    reference: &str,
) -> Result<GitOutput, AppError> {
    git.run(repository, &["stash", "drop", reference])
        .await
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_stash_records_unicode_and_empty() {
        let hash = "a".repeat(40);
        let input = format!("stash@{{0}}\0{hash}\0On main: 修复 space\02026-01-01T00:00:00Z\0\n");
        let list = parse_stashes(input.as_bytes()).unwrap();
        assert_eq!(list.stashes.len(), 1);
        assert_eq!(list.stashes[0].message, "On main: 修复 space");
        assert_eq!(resolve(&list, &hash).unwrap().reference, "stash@{0}");
        assert!(parse_stashes(b"").unwrap().stashes.is_empty());
        assert!(parse_stashes(b"broken\n").is_err());
    }
    #[test]
    fn duplicate_hash_is_ambiguous() {
        let hash = "b".repeat(40);
        let list = StashList {
            stashes: vec![
                StashInfo {
                    reference: "stash@{0}".into(),
                    commit_hash: hash.clone(),
                    message: "a".into(),
                    timestamp: "".into(),
                },
                StashInfo {
                    reference: "stash@{1}".into(),
                    commit_hash: hash.clone(),
                    message: "b".into(),
                    timestamp: "".into(),
                },
            ],
        };
        assert_eq!(resolve(&list, &hash).unwrap_err().code, "stash_ambiguous");
    }
}
