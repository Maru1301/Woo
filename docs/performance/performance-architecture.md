# Performance architecture

M1 kept Git and parsing off the UI thread using an asynchronous Tauri command and Tokio process handling. Repository open runs a bounded number of short Git commands. M2 uses a 60 second timeout for repository session operations. The open response contains only path, branch, HEAD metadata, and an elapsed time. There is no history load, diff load, cache, or filesystem watcher.

M2 status uses one Git process and a separate Rust parser. The service records Git process duration, parser duration, and total status duration. Mutation commands return a refreshed status, avoiding unrelated repository reloads. The frontend renders at most a small visible slice of each changed-file group while retaining the complete structured status in state. Status output is currently buffered by the process runner; revisit memory bounds if very large change sets show a measurable problem.

M3 commit uses four Git processes in the normal successful path: preflight status, commit, HEAD read, and post-commit status. The preflight protects user-facing validation against stale UI state. The HEAD read serves an existing UI field; status refresh is targeted to working-tree state. Commit duration can be dominated by hooks or signing and is not a useful micro-optimization target without workload-specific measurements.

Future milestones should refresh only state affected by working-tree, index, HEAD, or ref changes. Git filesystem notifications should be hints followed by validation. Add bounded caches only with explicit keys and invalidation. Measure open, status, history, graph, and diff separately before optimizing.
# M4 history

History requests ask Git for 100 commits plus one lookahead commit. Later pages include one predecessor commit to verify continuity. No request asks Git to materialize complete history output. The React list renders a fixed overscanned window of rows, so DOM row count stays bounded as pages accumulate. The loaded commit array itself grows with user browsing and has no eviction yet.

The initial 100-row page is independent of repository opening and status loading. `Git.Log` logs process, parsing, total time, returned rows, and raw output bytes. The benchmark also measures JSON serialization. Git traversal and process overhead dominated the measured small and medium fixtures; the next performance decision should use larger and merge-heavy data before changing parsing or pagination.

Offset pagination with a predecessor hash is simple and bounded in output, but Git may walk skipped commits again for later pages. It detects a shifted page boundary after ref changes; it is not a frozen snapshot across all refs. When continuity fails, the UI offers a reload. This is an intentional M4 tradeoff rather than a graph-ready persistent history cache.

# M5 diff

Diffs load only for a selected file. The Git runner limits patch stdout to 2 MiB and kills an over-limit child; the Rust parser rejects more than 20,000 source diff lines. Commit selection loads only a bounded NUL-delimited changed-file list, not patches. The frontend virtualizes both commit-file choices and diff rows. Binary diffs return a flag and are never rendered as text.

`Git.Diff` records Git process time, parser time, total time, and raw patch bytes. The benchmark measures JSON serialization separately, since structured line objects expand substantially at the Tauri boundary. For a 10,000-line text diff, serialization approached Git time in the debug baseline. The benchmark does not measure WebView IPC or browser layout/paint; those are open measurement tasks before raising the safeguards or adding streaming.

# M6 branches

One `for-each-ref` process enumerates local and remote-tracking refs, including upstream and current-branch metadata. Rust parsing remained under 2 ms for the median 1,000-ref sample; Git/ref access dominated. The list is loaded independently of repository opening, and both local and remote groups render fixed visible row windows. Creating a branch runs two Git processes: create, then list. Switching runs four: switch, list, HEAD, status. It resets history to one initial page and clears diff selection instead of eagerly loading old pages or patches. See `benchmark-plan.md` for timings and their filesystem variance.

# M7 graph

Graph layout is incremental and pure Rust. The history cursor carries active lane ownership, so each 100-row page is laid out once. No new Git process is used. The row payload contains only lane geometry and stays separate from commit metadata. The frontend draws one SVG per visible history row; the shared virtualized list keeps graph DOM count bounded as pages accumulate. The graph gutter caps at 12 visible lanes to protect commit text in pathological histories. The full topology remains in the layout/cursor data. Synthetic 10,000-commit benchmarks showed sub-1.1 ms worst page layout among measured cases; Git history traversal remained dominant. WebView paint, scrolling frame time, and Tauri IPC were not available to measure in M7's environment.
