# Frontend state boundaries

The frontend lives in `frontend/`. It keeps Git-owned repository facts separate from interaction state. `frontend/src/lib/repository.ts` remains the semantic Tauri API; React does not parse Git output.

| State | Current owner | Examples |
| --- | --- | --- |
| Workspace registry | `frontend/src/App.tsx`, backed by Rust `workspace.rs` | Workspace list, registered repository paths, active IDs; persisted as app configuration |
| Repository session | `frontend/src/app/repository-session/useRepositorySession.ts` | Open repository and HEAD, status, branches, active operation, conflicts |
| View | `App.tsx` and the relevant feature panel | Selected working-tree diff, selected history commit, active detail, refresh tokens |
| Layout | `frontend/src/layout/WorkspaceLayout.tsx` and `frontend/src/layout/workspace.css` | Detail position and size preset; both affect presentation only |
| Transient UI | The component that displays it | Dialogs, notices, pending confirmation, local form text, busy feedback |

Repository session updates use semantic reducer actions. A Git mutation result can update related repository facts in one transition. The shell still coordinates asynchronous operations and invalidates selected views explicitly. Feature panels keep local interaction state until a concrete cross-panel need calls for lifting it.

The Prototype 07 shell shows the active workspace's registered repositories as tabs. Every tab transition invokes the persisted workspace domain and replaces Woo's one active Git session; the tabs do not cache independent repository state. The sidebar's active tool, List/Tree choices for local and commit files, open popovers, context menus, and detail sizing are presentation state. The existing `App.tsx` still coordinates semantic mutation results and targeted invalidation; future refactors can extract those workflows one at a time without changing Git ownership.
