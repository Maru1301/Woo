# ADR 0004: Git-owned history operation state

Status: accepted for M11.

Woo extends M10's `RepositoryOperationState` with rebase, cherry-pick, and revert. Git's own marker files and rebase directories determine the active workflow; index stages determine unresolved files independently. The same conflict resolver and stage operations work across these workflows. A nonzero Git exit always leads to a repository-state read because Git may have entered a valid stopped operation. Rebase continuation may stop repeatedly, so each continue/skip result is classified from the next Git state.

The apply rebase backend shares `rebase-apply` with `git am`. Woo requires Git's `rebasing` marker there; an `am` session is reported as unsupported rather than offered a misleading `rebase --continue` action. Git's [am implementation](https://github.com/git/git/blob/master/builtin/am.c) uses that marker to distinguish rebasing.

The UI calls semantic Rust operations, never arbitrary revision commands. Rebase accepts an existing local branch. Cherry-pick, revert, and reset accept a validated complete commit hash. Merge-commit cherry-pick/revert require mainline semantics and are refused in M11. A temporary `core.editor=true` setting for continue/skip prevents hidden editor prompts while retaining normal hooks, signing, and system Git configuration.

During rebase, Git's stage 2/“ours” is the branch being rebased onto plus commits already replayed; stage 3/“theirs” is the commit currently replayed from the user's branch. Woo keeps these index-stage identities but explains the roles next to the side controls. See [Git checkout](https://git-scm.com/docs/git-checkout) and [Git rebase](https://git-scm.com/docs/git-rebase).

Hard reset is explicitly confirmed because Git may overwrite tracked changes and obstructing untracked files. Woo checks non-index files, including ignored paths, against paths tracked in the target commit and refuses a collision before invoking `git reset --hard`; it never runs `git clean`. This is a safety preflight, not an atomic guarantee against external filesystem changes between inspection and Git execution. Soft and mixed follow system Git semantics. See [Git reset](https://git-scm.com/docs/git-reset).
