use crate::git::{GitOutput, GitRunError};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<GitRunError> for AppError {
    fn from(error: GitRunError) -> Self {
        match error {
            GitRunError::MissingExecutable => {
                Self::new("git_missing", "Git is not installed or is not on PATH.")
            }
            GitRunError::Timeout => Self::new("git_timeout", "Git took too long to respond."),
            GitRunError::OutputLimit => Self::new(
                "diff_too_large",
                "This patch is too large to display safely.",
            ),
            GitRunError::Cancelled => {
                Self::new("operation_cancelled", "The Git operation was cancelled.")
            }
            GitRunError::Io(cause) => Self::new("git_io", format!("Could not run Git: {cause}")),
        }
    }
}

pub fn git_failure(output: &GitOutput) -> AppError {
    let detail = String::from_utf8_lossy(&output.stderr);
    if detail.contains("index.lock") {
        return AppError::new(
            "repository_locked",
            "The repository index is locked. Finish the other Git operation and try again.",
        );
    }
    let explanation = if detail.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        detail
    };
    AppError::new(
        "git_failed",
        format!("Git operation failed: {}", explanation.trim()),
    )
}
