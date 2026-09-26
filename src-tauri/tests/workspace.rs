use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
use woo_lib::{
    working_tree::WorkingTree,
    workspace::{canonical_repository_path, WorkspaceCatalog, WorkspaceManager},
};

fn git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    dir
}

#[test]
fn workspace_crud_persists_and_never_touches_repository() {
    let settings = TempDir::new().unwrap();
    let repo = repository();
    fs::write(repo.path().join("user.txt"), "important").unwrap();
    let manager = WorkspaceManager::new(settings.path().to_owned());
    let mut catalog = manager.load().unwrap();
    let first = catalog.active_workspace_id.clone().unwrap();
    catalog.rename(&first, "Work").unwrap();
    catalog.create("Personal").unwrap();
    let second = catalog.active_workspace_id.clone().unwrap();
    let repo_id = catalog
        .add_repository(&second, repo.path().to_string_lossy().into_owned())
        .unwrap();
    manager.save(&catalog).unwrap();
    let manager_again = WorkspaceManager::new(settings.path().to_owned());
    let mut loaded = manager_again.load().unwrap();
    assert_eq!(loaded, catalog);
    assert_eq!(loaded.active_path(), Some(repo.path().to_str().unwrap()));
    assert_eq!(
        loaded
            .add_repository(&second, repo.path().to_string_lossy().into_owned())
            .unwrap_err()
            .code,
        "repository_duplicate"
    );
    loaded.remove_repository(&second, &repo_id).unwrap();
    loaded.delete(&second).unwrap();
    assert_eq!(loaded.active_workspace_id.as_deref(), Some(first.as_str()));
    manager_again.save(&loaded).unwrap();
    assert_eq!(
        fs::read_to_string(repo.path().join("user.txt")).unwrap(),
        "important"
    );
    assert!(repo.path().join(".git").exists());
}

#[test]
fn invalid_settings_are_reported_without_overwrite() {
    let settings = TempDir::new().unwrap();
    let file = settings.path().join("workspaces.json");
    fs::write(&file, "{not json").unwrap();
    let manager = WorkspaceManager::new(settings.path().to_owned());
    assert_eq!(manager.load().unwrap_err().code, "workspace_config_invalid");
    assert_eq!(fs::read_to_string(&file).unwrap(), "{not json");
    assert_eq!(WorkspaceCatalog::default().workspaces.len(), 1);
}

#[tokio::test]
async fn validation_canonicalization_and_switch_invalidate_old_session() {
    let a = repository();
    let b = repository();
    let nested = a.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let canonical = canonical_repository_path(a.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        canonical,
        canonical_repository_path(nested.to_str().unwrap())
            .await
            .unwrap()
    );
    let mut catalog = WorkspaceCatalog::default();
    let workspace_id = catalog.active_workspace_id.clone().unwrap();
    catalog
        .add_repository(&workspace_id, canonical.clone())
        .unwrap();
    assert_eq!(
        catalog
            .add_repository(
                &workspace_id,
                canonical_repository_path(nested.to_str().unwrap())
                    .await
                    .unwrap()
            )
            .unwrap_err()
            .code,
        "repository_duplicate"
    );
    let other = TempDir::new().unwrap();
    assert_eq!(
        canonical_repository_path(other.path().to_str().unwrap())
            .await
            .unwrap_err()
            .code,
        "invalid_repository"
    );
    let tree = WorkingTree::default();
    let first = tree.open(&canonical).await.unwrap();
    let second = tree.open(b.path().to_str().unwrap()).await.unwrap();
    assert_ne!(first.session_id, second.session_id);
    assert_eq!(
        tree.watch_snapshot(first.session_id, true, true, true, None)
            .await
            .unwrap_err()
            .code,
        "repository_changed"
    );
    tree.close().await;
    assert_eq!(
        tree.watch_snapshot(second.session_id, true, true, true, None)
            .await
            .unwrap_err()
            .code,
        "repository_changed"
    );
    assert_eq!(tree.status().await.unwrap_err().code, "no_repository");
}

#[test]
fn switching_and_missing_path_are_configuration_only() {
    let mut catalog = WorkspaceCatalog::default();
    let first = catalog.active_workspace_id.clone().unwrap();
    assert_eq!(
        catalog
            .add_repository(&first, "relative/path".into())
            .unwrap_err()
            .code,
        "invalid_path"
    );
    let missing = TempDir::new().unwrap().path().join("gone");
    catalog
        .add_repository(&first, missing.to_string_lossy().into_owned())
        .unwrap();
    let repo = catalog.workspaces[0].active_repository_id.clone().unwrap();
    catalog.create("Second").unwrap();
    assert!(catalog.active_path().is_none());
    catalog.select_repository(&first, &repo).unwrap();
    assert_eq!(catalog.active_path(), missing.to_str());
    assert_eq!(
        catalog
            .select_repository(&first, "unknown")
            .unwrap_err()
            .code,
        "workspace_repository_not_found"
    );
}
