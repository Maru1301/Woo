use crate::{error::AppError, working_tree::WorkingTree, workspace::WorkspaceManager};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, OnceCell};

type SessionCell = Arc<OnceCell<Arc<WorkingTree>>>;

/// Resolves only repository IDs persisted in Woo's workspace catalog. Each
/// repository owns its own WorkingTree, so a UI selection cannot retarget an
/// operation that is already in flight.
pub struct RepositoryRegistry {
    workspace: Arc<WorkspaceManager>,
    sessions: Mutex<HashMap<String, (String, SessionCell)>>,
    mutation_gate: Arc<Mutex<()>>,
}

impl RepositoryRegistry {
    pub fn new(workspace: Arc<WorkspaceManager>, mutation_gate: Arc<Mutex<()>>) -> Self {
        Self {
            workspace,
            sessions: Mutex::new(HashMap::new()),
            mutation_gate,
        }
    }

    pub async fn resolve(&self, repository_id: &str) -> Result<Arc<WorkingTree>, AppError> {
        let catalog = self.workspace.load()?;
        let path = catalog
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.repositories)
            .find(|repository| repository.id == repository_id)
            .map(|repository| repository.path.clone())
            .ok_or_else(|| {
                AppError::new(
                    "workspace_repository_not_found",
                    "Repository is not registered in Woo.",
                )
            })?;
        let cell = {
            let mut sessions = self.sessions.lock().await;
            let entry = sessions
                .entry(repository_id.to_owned())
                .or_insert_with(|| (path.clone(), Arc::new(OnceCell::new())));
            if entry.0 != path {
                *entry = (path.clone(), Arc::new(OnceCell::new()));
            }
            Arc::clone(&entry.1)
        };
        let repository_id = repository_id.to_owned();
        let tree = cell
            .get_or_try_init(|| async {
                let tree = Arc::new(WorkingTree::with_mutation_gate(
                    repository_id,
                    Arc::clone(&self.mutation_gate),
                ));
                tree.open(&path).await?;
                Ok::<_, AppError>(tree)
            })
            .await?;
        Ok(Arc::clone(tree))
    }

    pub async fn operation_history(&self) -> Vec<crate::operation_log::OperationEntry> {
        let trees: Vec<_> = self
            .sessions
            .lock()
            .await
            .values()
            .filter_map(|(_, cell)| cell.get().cloned())
            .collect();
        let mut entries = Vec::new();
        for tree in trees {
            entries.extend(tree.operation_history().await);
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.started_at_ms));
        entries
    }
}
