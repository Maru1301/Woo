use crate::{
    repository::RepositoryInfo,
    working_tree::{RemoteOperationStatus, RemotePhase, RemoteRefresh, WorkingTree},
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::{sync::Mutex, task::JoinHandle};

pub const FETCH_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundFetchEvent {
    pub session_id: u64,
    pub operation_id: u64,
    pub refresh: Option<RemoteRefresh>,
}

#[derive(Default)]
pub struct BackgroundFetchManager {
    active: Mutex<Option<JoinHandle<()>>>,
}

impl BackgroundFetchManager {
    pub async fn stop(&self, tree: &WorkingTree) {
        if let Some(task) = self.active.lock().await.take() {
            task.abort();
        }
        tree.cancel_background_remote().await;
    }

    pub async fn activate(&self, app: AppHandle, tree: Arc<WorkingTree>, info: &RepositoryInfo) {
        self.stop(&tree).await;
        let session_id = info.session_id;
        let task = tokio::spawn(async move {
            loop {
                wait_interval().await;
                let mut completions = tree.remote_completions();
                let started = match tree.start_background_fetch(session_id).await {
                    Ok(Some(started)) => started,
                    Ok(None) => continue,
                    Err(error) => {
                        eprintln!("Background fetch admission: {}", error.code);
                        continue;
                    }
                };
                if let Some(done) = wait_for_completion(&mut completions, started.id).await {
                    if tree.active_session_id() == session_id {
                        let _ = app.emit(
                            "background-fetch-completed",
                            BackgroundFetchEvent {
                                session_id,
                                operation_id: done.id,
                                refresh: done.refresh,
                            },
                        );
                    }
                }
            }
        });
        *self.active.lock().await = Some(task);
    }
}

async fn wait_interval() {
    tokio::time::sleep(FETCH_INTERVAL).await;
}

async fn wait_for_completion(
    completions: &mut tokio::sync::broadcast::Receiver<RemoteOperationStatus>,
    id: u64,
) -> Option<RemoteOperationStatus> {
    loop {
        match completions.recv().await {
            Ok(status)
                if status.id == id
                    && matches!(
                        status.phase,
                        RemotePhase::Completed
                            | RemotePhase::Failed
                            | RemotePhase::Cancelled
                            | RemotePhase::TimedOut
                    ) =>
            {
                return Some(status)
            }
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn first_fetch_waits_ten_minutes() {
        let tick = tokio::spawn(wait_interval());
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(599)).await;
        assert!(!tick.is_finished());
        tokio::time::advance(Duration::from_secs(1)).await;
        tick.await.unwrap();
    }
}
