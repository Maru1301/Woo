# Prototype 07 production UI map

| Production owner | Prototype 07 destination | Preserved behavior |
| --- | --- | --- |
| `WorkspacePanel` and workspace commands | Top bar Workspace menu and repository tabs | Catalog persistence, registration/removal, single active session, watcher lifecycle |
| `RemotePanel` | Top bar Fetch/Pull/Push | Managed remote operation, cancellation, targeted refresh |
| `OperationHistory` | Top bar Activity popover | Bounded User/Background log and real operation states |
| `HistoryPanel` and `GraphRowView` | History primary view and contextual detail | Rust topology, virtual rows, paging, commit and diff selection |
| `FileGroup` / `DiffViewer` | Changes primary view | Status, staging, partial staging, commit and on-demand diff |
| `BranchPanel` | Sidebar references and Branches tool | Local/remote distinction, checkout, create/rename/safe delete |
| `TagPanel` / `StashPanel` | Tags and Stashes tools | Existing mutation safety and targeted refresh |
| `MergePanel` | Bottom Merge / Conflicts entry | Git-owned operation state, conflict resolution, continue/abort |

Changes and History are exclusive primary views. Branches, Tags, Stashes, and Merge / Conflicts are separate tool views; their existing components retain their production state and lifecycle while inactive views are hidden. Workspace and Activity appear as compact overlays rather than permanent vertical panels. File List/Tree choices are presentation state scoped separately to local and commit files. Context menus call existing semantic application handlers; actions unsupported by the production backend remain disabled rather than issuing raw Git commands.
