use crate::{error::AppError, remotes::redact_diagnostic};
use serde::Serialize;
use std::{
    collections::VecDeque,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub trait OperationOutcome {
    fn semantic_error(&self) -> Option<&AppError> {
        None
    }
}

macro_rules! success_outcome { ($($ty:ty),* $(,)?) => { $(impl OperationOutcome for $ty {})* }; }
success_outcome!(
    crate::status::RepositoryStatus,
    crate::branches::BranchList,
    crate::working_tree::CheckoutResult,
    crate::working_tree::CommitResult,
    crate::working_tree::BranchRefMutationResult,
    crate::working_tree::TagMutationResult
);
macro_rules! error_outcome { ($($ty:ty),* $(,)?) => { $(impl OperationOutcome for $ty {
    fn semantic_error(&self) -> Option<&AppError> { self.error.as_ref() }
})* }; }
error_outcome!(
    crate::working_tree::PartialStageResult,
    crate::working_tree::MergeMutationResult,
    crate::working_tree::HistoryMutationResult,
    crate::working_tree::StashMutationResult,
    crate::working_tree::ConflictMutationResult
);

pub const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationSource {
    User,
    Background,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEntry {
    pub id: u64,
    pub repository_id: String,
    pub kind: String,
    pub source: OperationSource,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    pub duration_ms: Option<u128>,
    pub phase: OperationPhase,
    pub summary: String,
    pub diagnostics: Option<String>,
}

#[derive(Default)]
pub struct OperationLog {
    entries: Mutex<VecDeque<OperationEntry>>,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn diagnostic(error: &AppError) -> String {
    // The log is more conservative than the immediate operation error: never
    // retain a URL, even when it did not originate from remote configuration.
    redact_diagnostic(&error.message)
        .chars()
        .take(2048)
        .collect()
}

impl OperationLog {
    pub async fn begin(&self, id: u64, repository_id: String, kind: &str, source: OperationSource) {
        let mut entries = self.entries.lock().await;
        if entries.len() == HISTORY_LIMIT {
            entries.pop_back();
        }
        entries.push_front(OperationEntry {
            id,
            repository_id,
            kind: kind.to_owned(),
            source,
            started_at_ms: now_ms(),
            finished_at_ms: None,
            duration_ms: None,
            phase: OperationPhase::Running,
            summary: format!("{kind} in progress"),
            diagnostics: None,
        });
    }

    pub async fn finish(&self, id: u64, started: Instant, error: Option<&AppError>) {
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
            entry.finished_at_ms = Some(now_ms());
            entry.duration_ms = Some(started.elapsed().as_millis());
            entry.phase = match error.map(|error| error.code) {
                Some("operation_cancelled") => OperationPhase::Cancelled,
                Some("git_timeout") => OperationPhase::TimedOut,
                Some(_) => OperationPhase::Failed,
                None => OperationPhase::Completed,
            };
            entry.summary = format!(
                "{} {}",
                entry.kind,
                match entry.phase {
                    OperationPhase::Completed => "complete",
                    OperationPhase::Cancelled => "cancelled",
                    OperationPhase::TimedOut => "timed out",
                    _ => "failed",
                }
            );
            entry.diagnostics = error.map(diagnostic);
        }
    }

    pub async fn list(&self) -> Vec<OperationEntry> {
        self.entries.lock().await.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounds_history_and_redacts_failure() {
        let log = OperationLog::default();
        for id in 1..=105 {
            log.begin(id, "repo".into(), "Fetch", OperationSource::Background)
                .await;
        }
        assert_eq!(log.list().await.len(), HISTORY_LIMIT);
        assert_eq!(log.list().await.last().unwrap().id, 6);
        log.finish(
            105,
            Instant::now(),
            Some(&AppError::new(
                "remote_failed",
                "https://user:secret@example.com/path?token=x failed",
            )),
        )
        .await;
        let recent = log.list().await.remove(0);
        assert_eq!(recent.phase, OperationPhase::Failed);
        assert!(!recent.diagnostics.unwrap().contains("secret"));
    }

    #[tokio::test]
    async fn cancellation_and_timeout_have_distinct_terminal_states() {
        let log = OperationLog::default();
        log.begin(1, "repo".into(), "Fetch", OperationSource::Background)
            .await;
        log.begin(2, "repo".into(), "Fetch", OperationSource::Background)
            .await;
        assert_eq!(log.list().await[0].phase, OperationPhase::Running);
        log.finish(
            1,
            Instant::now(),
            Some(&AppError::new("operation_cancelled", "Cancelled")),
        )
        .await;
        log.finish(
            2,
            Instant::now(),
            Some(&AppError::new("git_timeout", "Timed out")),
        )
        .await;
        let entries = log.list().await;
        assert_eq!(entries[0].phase, OperationPhase::TimedOut);
        assert_eq!(entries[1].phase, OperationPhase::Cancelled);
    }
}
