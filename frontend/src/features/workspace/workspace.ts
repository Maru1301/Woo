import { invoke } from "@tauri-apps/api/core";
import type { AppError, RepositoryInfo } from "../../lib/repository";

export interface WorkspaceRepository { id: string; path: string }
export interface Workspace { id: string; name: string; repositories: WorkspaceRepository[]; activeRepositoryId: string | null }
export interface WorkspaceCatalog { version: number; nextId: number; workspaces: Workspace[]; activeWorkspaceId: string | null }
export interface WorkspaceTransition { catalog: WorkspaceCatalog; repository: RepositoryInfo | null; openError: AppError | null; sessionChanged: boolean }

export function restoreWorkspaceSession(): Promise<WorkspaceTransition> { return invoke("restore_workspace_session"); }
export function createWorkspace(name: string): Promise<WorkspaceTransition> { return invoke("create_workspace", { name }); }
export function renameWorkspace(id: string, name: string): Promise<WorkspaceCatalog> { return invoke("rename_workspace", { id, name }); }
export function deleteWorkspace(id: string): Promise<WorkspaceTransition> { return invoke("delete_workspace", { id }); }
export function registerRepository(workspaceId: string, path: string): Promise<WorkspaceTransition> { return invoke("register_workspace_repository", { workspaceId, path }); }
export function removeRepository(workspaceId: string, repositoryId: string): Promise<WorkspaceTransition> { return invoke("remove_workspace_repository", { workspaceId, repositoryId }); }
export function switchWorkspace(id: string): Promise<WorkspaceTransition> { return invoke("switch_workspace", { id }); }
export function switchWorkspaceRepository(workspaceId: string, repositoryId: string): Promise<WorkspaceTransition> { return invoke("switch_workspace_repository", { workspaceId, repositoryId }); }
export function selectWorkspaceRepository(workspaceId: string, repositoryId: string): Promise<WorkspaceCatalog> { return invoke("select_workspace_repository", { workspaceId, repositoryId }); }
export function activateRepositoryWatch(repositoryId: string): Promise<RepositoryInfo> { return invoke("activate_repository_watch", { repositoryId }); }
