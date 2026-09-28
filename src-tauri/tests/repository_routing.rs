use std::{fs, path::Path, process::Command, sync::Arc};
use tempfile::TempDir;
use tokio::sync::Mutex;
use woo_lib::{
    repository_registry::RepositoryRegistry,
    working_tree::{RemoteKind, RemotePhase},
    workspace::WorkspaceManager,
};

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn repository() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    dir
}

fn path(directory: &Path) -> String {
    directory.to_string_lossy().into_owned()
}

#[tokio::test]
async fn push_after_resolving_another_repository_still_targets_requested_id() {
    let settings = TempDir::new().unwrap();
    let a = repository();
    let b = repository();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    for repo in [&a, &b] {
        git(repo.path(), &["config", "user.name", "Woo Test"]);
        git(repo.path(), &["config", "user.email", "woo@example.test"]);
        fs::write(repo.path().join("file.txt"), "initial").unwrap();
        git(repo.path(), &["add", "file.txt"]);
        git(repo.path(), &["commit", "-m", "initial"]);
    }
    git(b.path(), &["remote", "add", "origin", &path(remote.path())]);
    git(b.path(), &["push", "-u", "origin", "main"]);
    fs::write(b.path().join("file.txt"), "next").unwrap();
    git(b.path(), &["commit", "-am", "next"]);
    let expected = git(b.path(), &["rev-parse", "HEAD"]);

    let manager = Arc::new(WorkspaceManager::new(settings.path().to_owned()));
    let mut catalog = manager.load().unwrap();
    let workspace_id = catalog.active_workspace_id.clone().unwrap();
    let a_id = catalog
        .add_repository(&workspace_id, path(a.path()))
        .unwrap();
    let b_id = catalog
        .add_repository(&workspace_id, path(b.path()))
        .unwrap();
    manager.save(&catalog).unwrap();
    let registry = RepositoryRegistry::new(manager, Arc::new(Mutex::new(())));
    registry.resolve(&a_id).await.unwrap();
    let target = registry.resolve(&b_id).await.unwrap();
    let started = target.start_remote(RemoteKind::Push, None).await.unwrap();
    let finished = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let status = target.remote_status(started.id).await.unwrap();
            if matches!(
                status.phase,
                RemotePhase::Completed
                    | RemotePhase::Failed
                    | RemotePhase::Cancelled
                    | RemotePhase::TimedOut
            ) {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(finished.phase, RemotePhase::Completed),
        "{:?}",
        finished.error
    );
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), expected);
    assert_eq!(registry.operation_history().await[0].repository_id, b_id);
}

#[tokio::test]
async fn repository_ids_route_mutations_without_changing_other_sessions() {
    let settings = TempDir::new().unwrap();
    let a = repository();
    let b = repository();
    fs::write(a.path().join("a.txt"), "A").unwrap();
    fs::write(b.path().join("b.txt"), "B").unwrap();
    let manager = Arc::new(WorkspaceManager::new(settings.path().to_owned()));
    let mut catalog = manager.load().unwrap();
    let workspace_id = catalog.active_workspace_id.clone().unwrap();
    let a_id = catalog
        .add_repository(&workspace_id, path(a.path()))
        .unwrap();
    let b_id = catalog
        .add_repository(&workspace_id, path(b.path()))
        .unwrap();
    manager.save(&catalog).unwrap();
    let registry = RepositoryRegistry::new(manager, Arc::new(Mutex::new(())));

    let a_tree = registry.resolve(&a_id).await.unwrap();
    let b_tree = registry.resolve(&b_id).await.unwrap();
    assert_ne!(
        a_tree.info().await.unwrap().session_id,
        b_tree.info().await.unwrap().session_id
    );
    assert_eq!(
        registry.resolve("not-registered").await.err().unwrap().code,
        "workspace_repository_not_found"
    );

    b_tree
        .logged_user("Stage all", b_tree.stage_all())
        .await
        .unwrap();
    let b_status = b_tree.status().await.unwrap();
    let a_status = a_tree.status().await.unwrap();
    assert_eq!(b_status.staged.len(), 1);
    assert_eq!(b_status.staged[0].path, "b.txt");
    assert_eq!(a_status.untracked.len(), 1);
    assert_eq!(a_status.untracked[0].path, "a.txt");
    assert!(a_status.staged.is_empty());
    assert_eq!(registry.operation_history().await[0].repository_id, b_id);
}
