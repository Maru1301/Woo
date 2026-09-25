# ADR 0003: Git owns conflict state

Status: accepted for M10.

Woo reconstructs an active merge from `MERGE_HEAD` and unmerged file sides from index stages 1, 2, and 3. It does not persist its own resolved flags or infer conflicts from working-file markers or a merge command's exit code. A non-zero Git result can still produce a valid merge in progress. Every merge and resolution mutation therefore returns refreshed repository state, including on failure after Git may have changed files.

The application layer holds the existing repository mutation mutex through Git execution and refresh. React receives semantic operation/conflict models and can hold only selection and editor text. Conflict content is fetched on selection with bounded reads. Text saves compare the last shown working content before replacing the file; whole-side actions are explicit and stage through Git. Binary and oversized files retain whole-side resolution and external-edit options.

This adds one merge-marker Git query to a repository-state refresh, and an unmerged-index query only when status reports conflicts. That process overhead is accepted for restart correctness. Stage-shape conflict kinds can be refined for rename/custom-driver cases later without changing the source of truth. Future rebase, cherry-pick, revert, and stash workflows can reuse index-stage content and resolution operations while adding their own operation markers and continue/abort semantics.
