//! Woo-owned repository organization. Git state remains in `WorkingTree`.
use crate::{error::AppError, git::GitRunner, repository};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::sync::Mutex;

const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRepository {
    pub id: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub repositories: Vec<WorkspaceRepository>,
    pub active_repository_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCatalog {
    pub version: u32,
    pub next_id: u64,
    pub workspaces: Vec<Workspace>,
    pub active_workspace_id: Option<String>,
}

impl Default for WorkspaceCatalog {
    fn default() -> Self {
        Self {
            version: VERSION,
            next_id: 2,
            workspaces: vec![Workspace {
                id: "w1".into(),
                name: "Default".into(),
                repositories: Vec::new(),
                active_repository_id: None,
            }],
            active_workspace_id: Some("w1".into()),
        }
    }
}

fn clean_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::new(
            "workspace_name",
            "Enter a workspace name of 1–80 characters.",
        ));
    }
    Ok(name.to_owned())
}

fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\").to_lowercase()
    } else {
        path.to_owned()
    }
}

impl WorkspaceCatalog {
    fn id(&mut self, prefix: char) -> String {
        let id = format!("{prefix}{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn workspace_mut(&mut self, id: &str) -> Result<&mut Workspace, AppError> {
        self.workspaces
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| AppError::new("workspace_not_found", "Workspace not found."))
    }

    pub fn create(&mut self, name: &str) -> Result<(), AppError> {
        let name = clean_name(name)?;
        let id = self.id('w');
        self.workspaces.push(Workspace {
            id: id.clone(),
            name,
            repositories: Vec::new(),
            active_repository_id: None,
        });
        self.active_workspace_id = Some(id);
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), AppError> {
        self.workspace_mut(id)?.name = clean_name(name)?;
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), AppError> {
        let index = self
            .workspaces
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| AppError::new("workspace_not_found", "Workspace not found."))?;
        self.workspaces.remove(index);
        if self.active_workspace_id.as_deref() == Some(id) {
            self.active_workspace_id = self.workspaces.first().map(|item| item.id.clone());
        }
        Ok(())
    }

    pub fn add_repository(&mut self, workspace_id: &str, path: String) -> Result<String, AppError> {
        if !Path::new(&path).is_absolute() {
            return Err(AppError::new(
                "invalid_path",
                "Choose an absolute repository path.",
            ));
        }
        let key = path_key(&path);
        if self
            .workspace_mut(workspace_id)?
            .repositories
            .iter()
            .any(|item| path_key(&item.path) == key)
        {
            return Err(AppError::new(
                "repository_duplicate",
                "This repository is already in the workspace.",
            ));
        }
        let id = self.id('r');
        let workspace = self.workspace_mut(workspace_id)?;
        workspace.repositories.push(WorkspaceRepository {
            id: id.clone(),
            path,
        });
        workspace.active_repository_id = Some(id.clone());
        self.active_workspace_id = Some(workspace_id.to_owned());
        Ok(id)
    }

    pub fn remove_repository(
        &mut self,
        workspace_id: &str,
        repository_id: &str,
    ) -> Result<(), AppError> {
        let workspace = self.workspace_mut(workspace_id)?;
        let index = workspace
            .repositories
            .iter()
            .position(|item| item.id == repository_id)
            .ok_or_else(|| {
                AppError::new(
                    "workspace_repository_not_found",
                    "Repository is not registered in this workspace.",
                )
            })?;
        workspace.repositories.remove(index);
        if workspace.active_repository_id.as_deref() == Some(repository_id) {
            workspace.active_repository_id =
                workspace.repositories.first().map(|item| item.id.clone());
        }
        Ok(())
    }

    pub fn select_workspace(&mut self, id: &str) -> Result<(), AppError> {
        if !self.workspaces.iter().any(|item| item.id == id) {
            return Err(AppError::new("workspace_not_found", "Workspace not found."));
        }
        self.active_workspace_id = Some(id.to_owned());
        Ok(())
    }

    pub fn select_repository(
        &mut self,
        workspace_id: &str,
        repository_id: &str,
    ) -> Result<(), AppError> {
        let workspace = self.workspace_mut(workspace_id)?;
        if !workspace
            .repositories
            .iter()
            .any(|item| item.id == repository_id)
        {
            return Err(AppError::new(
                "workspace_repository_not_found",
                "Repository is not registered in this workspace.",
            ));
        }
        workspace.active_repository_id = Some(repository_id.to_owned());
        self.active_workspace_id = Some(workspace_id.to_owned());
        Ok(())
    }

    pub fn active_path(&self) -> Option<&str> {
        let workspace = self
            .workspaces
            .iter()
            .find(|item| Some(item.id.as_str()) == self.active_workspace_id.as_deref())?;
        workspace
            .repositories
            .iter()
            .find(|item| Some(item.id.as_str()) == workspace.active_repository_id.as_deref())
            .map(|item| item.path.as_str())
    }

    fn valid(&self) -> bool {
        if self.version != VERSION || self.next_id == 0 {
            return false;
        }
        if self.workspaces.is_empty() != self.active_workspace_id.is_none() {
            return false;
        }
        if self
            .active_workspace_id
            .as_ref()
            .is_some_and(|id| !self.workspaces.iter().any(|item| &item.id == id))
        {
            return false;
        }
        let mut ids = std::collections::HashSet::new();
        for workspace in &self.workspaces {
            if !ids.insert(&workspace.id) || workspace.name.trim().is_empty() {
                return false;
            }
            if workspace.repositories.is_empty() != workspace.active_repository_id.is_none() {
                return false;
            }
            if workspace
                .active_repository_id
                .as_ref()
                .is_some_and(|id| !workspace.repositories.iter().any(|item| &item.id == id))
            {
                return false;
            }
            let mut paths = std::collections::HashSet::new();
            for repository in &workspace.repositories {
                if !ids.insert(&repository.id)
                    || !Path::new(&repository.path).is_absolute()
                    || !paths.insert(path_key(&repository.path))
                {
                    return false;
                }
            }
        }
        if ids.iter().any(|id| {
            let number = id.get(1..).and_then(|text| text.parse::<u64>().ok());
            !matches!(id.chars().next(), Some('w' | 'r'))
                || number.is_none_or(|value| value >= self.next_id)
        }) {
            return false;
        }
        true
    }
}

pub struct WorkspaceManager {
    path: PathBuf,
    pub(crate) gate: Mutex<()>,
}

impl WorkspaceManager {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            path: config_dir.join("workspaces.json"),
            gate: Mutex::new(()),
        }
    }

    pub fn load(&self) -> Result<WorkspaceCatalog, AppError> {
        if let Ok(metadata) = fs::metadata(&self.path) {
            if metadata.len() > 2 * 1024 * 1024 {
                return Err(AppError::new(
                    "workspace_config_invalid",
                    "Workspace settings are too large. The file was preserved.",
                ));
            }
        }
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorkspaceCatalog::default())
            }
            Err(error) => {
                return Err(AppError::new(
                    "workspace_io",
                    format!("Could not read workspace settings: {error}"),
                ))
            }
        };
        let catalog: WorkspaceCatalog = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::new("workspace_config_invalid", "Workspace settings are malformed. The file was preserved; repair or move it to continue."))?;
        if !catalog.valid() {
            return Err(AppError::new("workspace_config_invalid", "Workspace settings are invalid. The file was preserved; repair or move it to continue."));
        }
        Ok(catalog)
    }

    pub fn save(&self, catalog: &WorkspaceCatalog) -> Result<(), AppError> {
        if !catalog.valid() {
            return Err(AppError::new(
                "workspace_config_invalid",
                "Cannot save invalid workspace settings.",
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| AppError::new("workspace_io", "Workspace settings path is invalid."))?;
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "workspace_io",
                format!("Could not create settings directory: {error}"),
            )
        })?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
            AppError::new(
                "workspace_io",
                format!("Could not write workspace settings: {error}"),
            )
        })?;
        serde_json::to_writer_pretty(&mut temp, catalog).map_err(|error| {
            AppError::new(
                "workspace_io",
                format!("Could not serialize workspace settings: {error}"),
            )
        })?;
        temp.write_all(b"\n")
            .and_then(|_| temp.flush())
            .map_err(|error| {
                AppError::new(
                    "workspace_io",
                    format!("Could not write workspace settings: {error}"),
                )
            })?;
        temp.persist(&self.path).map_err(|error| {
            AppError::new(
                "workspace_io",
                format!("Could not replace workspace settings: {}", error.error),
            )
        })?;
        Ok(())
    }
}

pub async fn canonical_repository_path(input: &str) -> Result<String, AppError> {
    let info = repository::open(&GitRunner::default(), input).await?;
    let path = dunce::canonicalize(Path::new(&info.path)).map_err(|error| {
        AppError::new(
            "repository_unavailable",
            format!("Could not resolve repository path: {error}"),
        )
    })?;
    Ok(path.to_string_lossy().into_owned())
}
