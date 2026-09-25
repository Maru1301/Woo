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
pub enum TagKind {
    Lightweight,
    Annotated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub name: String,
    pub target_hash: String,
    pub kind: TagKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagList {
    pub tags: Vec<TagInfo>,
}

#[derive(Debug)]
pub struct TagTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
}

const TAG_FORMAT: &str = "%(refname)%00%(objectname)%00%(objecttype)%00%(*objectname)";

fn malformed() -> AppError {
    AppError::new(
        "invalid_git_output",
        "Git returned malformed tag information.",
    )
}

pub fn parse_tags(bytes: &[u8]) -> Result<TagList, AppError> {
    let mut tags = Vec::new();
    for record in bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let fields: Vec<_> = record.split(|byte| *byte == 0).collect();
        if fields.len() != 4 {
            return Err(malformed());
        }
        let field = |index| std::str::from_utf8(fields[index]).map_err(|_| malformed());
        let name = field(0)?.strip_prefix("refs/tags/").ok_or_else(malformed)?;
        let object = field(1)?;
        let (kind, target) = match field(2)? {
            "tag" => (TagKind::Annotated, field(3)?),
            _ => (TagKind::Lightweight, object),
        };
        if name.is_empty() || target.is_empty() {
            return Err(malformed());
        }
        tags.push(TagInfo {
            name: name.to_owned(),
            target_hash: target.to_owned(),
            kind,
        });
    }
    Ok(TagList { tags })
}

pub async fn load_tags(
    git: &GitRunner,
    repository: &Path,
) -> Result<(TagList, TagTiming), AppError> {
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
                TAG_FORMAT,
                "refs/tags",
            ],
        )
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let tags = parse_tags(&output.stdout)?;
    let timing = TagTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
    };
    eprintln!(
        "Git.Tags git_ms={} parse_us={} total_ms={} count={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis(),
        tags.tags.len()
    );
    Ok((tags, timing))
}

pub fn validate_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.as_bytes().contains(&0)
        || name.len() > 1024
        || name.starts_with('-')
    {
        return Err(AppError::new("invalid_tag_name", "Enter a valid tag name."));
    }
    Ok(())
}

pub async fn create_tag(
    git: &GitRunner,
    repository: &Path,
    name: &str,
    target: &str,
    annotation: Option<&str>,
) -> Result<Duration, AppError> {
    validate_name(name)?;
    let output = if let Some(message) = annotation {
        if message.trim().is_empty() {
            return Err(AppError::new(
                "empty_tag_message",
                "Enter an annotation message.",
            ));
        }
        if message.as_bytes().contains(&0) || message.len() > 1024 * 1024 {
            return Err(AppError::new(
                "invalid_tag_message",
                "The annotation message is too large or contains unsupported content.",
            ));
        }
        git.run_with_input(
            repository,
            &[
                "tag",
                "-a",
                "--cleanup=verbatim",
                "-F",
                "-",
                "--",
                name,
                target,
            ],
            Some(message.as_bytes()),
        )
        .await
        .map_err(AppError::from)?
    } else {
        git.run(repository, &["tag", "--", name, target])
            .await
            .map_err(AppError::from)?
    };
    if !output.success() {
        return Err(git_failure(&output));
    }
    Ok(output.duration)
}

pub async fn delete_tag(
    git: &GitRunner,
    repository: &Path,
    name: &str,
) -> Result<Duration, AppError> {
    validate_name(name)?;
    let output = git
        .run(repository, &["tag", "-d", "--", name])
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
    fn parses_lightweight_annotated_unicode_and_empty() {
        let tags =
            parse_tags("refs/tags/初版\0aaa\0commit\0\nrefs/tags/v2\0bbb\0tag\0ccc\n".as_bytes())
                .unwrap();
        assert_eq!(tags.tags.len(), 2);
        assert_eq!(tags.tags[0].name, "初版");
        assert_eq!(tags.tags[0].kind, TagKind::Lightweight);
        assert_eq!(tags.tags[1].target_hash, "ccc");
        assert_eq!(tags.tags[1].kind, TagKind::Annotated);
        assert!(parse_tags(b"").unwrap().tags.is_empty());
        assert!(parse_tags(b"broken\n").is_err());
    }
}
