use crate::{
    error::{git_failure, AppError},
    git::{GitOutput, GitRunner},
};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadInfo {
    pub hash: String,
    pub subject: String,
    pub author_date: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<HeadInfo>,
    pub open_duration_ms: u128,
}

fn parse_head(output: &GitOutput) -> Result<HeadInfo, AppError> {
    let parts: Vec<_> = output.stdout.split(|byte| *byte == 0).collect();
    if parts.len() < 3 {
        return Err(AppError::new(
            "invalid_git_output",
            "Git returned incomplete HEAD information.",
        ));
    }
    let field = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .trim_end_matches(['\r', '\n'])
            .to_string()
    };
    Ok(HeadInfo {
        hash: field(parts[0]),
        subject: field(parts[1]),
        author_date: field(parts[2]),
    })
}

pub async fn read_head(
    git: &GitRunner,
    repository: &Path,
) -> Result<(HeadInfo, Duration), AppError> {
    let output = git
        .run(repository, &["log", "-1", "--format=%H%x00%s%x00%aI"])
        .await
        .map_err(AppError::from)?;
    if !output.success() {
        return Err(git_failure(&output));
    }
    Ok((parse_head(&output)?, output.duration))
}

pub async fn open(git: &GitRunner, input: &str) -> Result<RepositoryInfo, AppError> {
    let started = Instant::now();
    let directory = Path::new(input);
    if !directory.is_dir() {
        return Err(AppError::new(
            "path_not_found",
            "Choose an existing directory.",
        ));
    }

    let inside = git
        .run(directory, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map_err(AppError::from)?;
    if !inside.success() || inside.stdout_text() != "true" {
        return Err(AppError::new(
            "invalid_repository",
            "This directory is not inside a Git working tree.",
        ));
    }
    let root = git
        .run(directory, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(AppError::from)?;
    if !root.success() {
        return Err(git_failure(&root));
    }
    let path = root.stdout_text();
    if path.is_empty() {
        return Err(AppError::new(
            "invalid_git_output",
            "Git returned an empty repository path.",
        ));
    }
    let root_directory = Path::new(&path);

    let branch_output = git
        .run(
            root_directory,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )
        .await
        .map_err(AppError::from)?;
    let branch = if branch_output.success() {
        Some(branch_output.stdout_text())
    } else if branch_output.exit_code == Some(1) {
        None
    } else {
        return Err(git_failure(&branch_output));
    };

    let head_output = git
        .run(root_directory, &["log", "-1", "--format=%H%x00%s%x00%aI"])
        .await
        .map_err(AppError::from)?;
    let head = if head_output.success() {
        Some(parse_head(&head_output)?)
    } else if head_output.exit_code == Some(128)
        && String::from_utf8_lossy(&head_output.stderr).contains("does not have any commits yet")
    {
        None
    } else {
        return Err(git_failure(&head_output));
    };

    eprintln!(
        "Repository.Open elapsed_ms={} path={}",
        started.elapsed().as_millis(),
        path
    );
    Ok(RepositoryInfo {
        path,
        branch,
        head,
        open_duration_ms: started.elapsed().as_millis(),
    })
}
