# Benchmark plan

M1 has `cargo run --example bench_open -- <path>`, which opens the same repository ten times in one process and reports minimum, median, and maximum elapsed microseconds. Use a generated temporary repository or record the repository size and machine when comparing runs. `Repository.Open` also logs elapsed milliseconds during normal use.

The measured open path includes directory validation and four Git processes (`rev-parse` twice, `symbolic-ref`, `log`). The example also times the standalone `rev-parse --is-inside-work-tree` process, which represents validation plus Git process overhead. These are local baselines, not comparisons with Fork.

M2 should add repeatable small/medium/large generated fixtures and measure status refresh with changed/untracked files. Later milestones can add history, graph, diff, UI responsiveness, and memory measurements.

## M1 baseline, 2026-09-24

Windows, debug build, local generated repository with one commit and one branch, ten samples in one process:

| Operation | Minimum | Median | Maximum |
| --- | ---: | ---: | ---: |
| Repository open (four Git processes) | 125.4 ms | 132.4 ms | 150.9 ms |
| Git validation process (`rev-parse`) | 27.3 ms | 28.8 ms | 29.5 ms |

Process launch plus Git startup accounts for a large share of this tiny-repository open time. These numbers include local machine and filesystem effects; they are not representative of large repositories or UI time, and do not establish a comparison with Fork.

## M2 baseline, 2026-09-24

`cargo run --example bench_status` generates a temporary repository with 1,000 tracked files, then measures clean, small-change (5 modified and 5 untracked), and larger-change (500 modified and 500 untracked) states. Ten debug-build samples were taken per state on Windows. Values are min/median/max in milliseconds.

| State | Git process | Rust parsing | Total status |
| --- | ---: | ---: | ---: |
| Clean | 119.478 / 122.137 / 125.379 | 0 / 0 / 0 | 119.565 / 122.225 / 125.452 |
| Small changes | 119.418 / 126.719 / 143.982 | 0.007 / 0.010 / 0.026 | 119.526 / 126.816 / 144.070 |
| Larger changes | 85.921 / 97.680 / 112.097 | 0.219 / 0.268 / 0.528 | 86.290 / 98.019 / 112.470 |

The larger state was faster than the other states in this run, likely reflecting cache and filesystem variation. The results do not establish scaling behavior. Parsing remained below 1 ms for the tested 1,000-change state. The benchmark does not measure Tauri serialization or React rendering.

## M3 commit timing, 2026-09-24

A single debug-build initial commit in a temporary Windows repository, measured by `Commit.Workflow` instrumentation during the focused integration test:

| Step | Duration | Git processes |
| --- | ---: | ---: |
| Preflight status | 173 ms | 1 |
| Git commit | 277 ms | 1 |
| HEAD read | 150 ms | 1 |
| Post-commit status | 186 ms | 1 |
| Total workflow | 788 ms | 4 |

This is one local observation, including process startup and filesystem effects. Hooks and signing configuration can change commit time substantially. No UI serialization/render timing or Fork comparison was measured.

## M4 history baseline, 2026-09-24

`cargo run --example bench_history` uses `git fast-import` to generate linear 1,000 and 10,000 commit repositories, then requests pages of 100 commits. Values below are from the median-total sample among five debug-build requests per page on this Windows workspace. Each page request is one Git process; the benchmark obtains earlier cursors separately before timing later page requests.

| Fixture | Page | Git process | Rust parse | Total request | JSON serialize | JSON bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 commits | 1 | 162.0 ms | 1.41 ms | 163.7 ms | 3.83 ms | 24,102 |
| 1,000 commits | 2 | 146.5 ms | 1.16 ms | 147.9 ms | 3.93 ms | 24,088 |
| 1,000 commits | 3 | 123.9 ms | 1.10 ms | 125.3 ms | 4.79 ms | 24,088 |
| 10,000 commits | 1 | 230.1 ms | 1.43 ms | 231.7 ms | 6.20 ms | 24,202 |
| 10,000 commits | 2 | 290.3 ms | 1.27 ms | 292.1 ms | 4.67 ms | 24,188 |
| 10,000 commits | 3 | 215.4 ms | 1.14 ms | 216.7 ms | 2.80 ms | 24,188 |

Fixture generation took 0.5 seconds for 1,000 and 1.9 seconds for 10,000 commits via one `fast-import` process. Git/process/traversal cost dominates parser and serialization cost in this sample. The benchmark does not measure Tauri IPC or React rendering, and the generated histories are linear with identical timestamps and tiny trees. Manual scrolling smoke testing is still needed; no Fork comparison is implied.

## M5 diff baseline, 2026-09-25

`cargo run --example bench_diff` generates separate temporary repositories with 100, 10,000, and 50,000 changed diff lines. Each fixture has one committed file, then a full-file working-tree replacement. Five debug-build requests were measured on Windows; values below come from the median-total sample. The benchmark serializes the structured Rust response with `serde_json`, which is a proxy for payload construction, not a measurement of Tauri IPC or WebView rendering.

| Fixture | Git process | Rust parse | Total Rust | JSON serialize | Raw patch | JSON payload |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 diff lines | 37.6 ms | 0.094 ms | 37.8 ms | 0.61 ms | 5.8 KB | 13.0 KB |
| 10,000 diff lines | 42.3 ms | 2.13 ms | 44.6 ms | 39.6 ms | 570 KB | 1.30 MB |
| 50,000 diff lines | — | — | rejected in 58.8 ms | — | exceeded guard | no payload |

The large fixture triggered `diff_too_large`; it did not enter the WebView. Git dominated the small request, while JSON serialization became nearly as expensive as Git for the medium response. No React frame or Tauri IPC measurement was available. The result supports keeping the current limits until UI and IPC costs are measured directly. There is no comparison with Fork.

## M6 branch baseline, 2026-09-25

`cargo run --example bench_branches` creates one commit and 20, 200, or 1,000 local refs pointing to it. Refs are generated in one `git update-ref --stdin` process and remain loose. Five debug-build branch requests were made per fixture on Windows; values below are the median-total sample from the second run after a parser refinement. Each request is one `for-each-ref` Git process.

| Local refs | Git process | Rust parse | Total branch request |
| --- | ---: | ---: | ---: |
| 20 | 40.853 ms | 0.049 ms | 40.911 ms |
| 200 | 91.988 ms | 0.338 ms | 92.335 ms |
| 1,000 | 310.282 ms | 1.343 ms | 311.635 ms |

An earlier run of the 1,000-ref fixture measured 741 ms total, showing substantial filesystem/cache variation in this workspace. Git/ref access dominates parsing. The same example measured one trivial checkout at 50.349 ms; this is diagnostic only and excludes Woo's post-checkout refresh. An integration checkout with two branches logged four processes (switch, refs, HEAD, status) and 581 ms total in one sample. The benchmark does not measure Tauri IPC or WebView rendering. No Fork comparison exists.

## M7 commit graph baseline, 2026-09-25

`cargo run --example bench_graph` uses pure in-memory topology, processes 100-commit pages, and round-trips the continuation token between pages. Five debug-build samples per fixture were taken on Windows; values below are median by total layout time. Layout time excludes token encoding and JSON serialization. The graph JSON column is one 100-row page, without commit metadata.

| Topology | Commits | Total layout | Slowest page | Graph JSON | First-page JSON serialization | Peak lanes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Linear | 1,000 | 1.830 ms | 0.234 ms | 8.2 KB | 0.289 ms | 1 |
| Linear | 10,000 | 20.264 ms | 0.307 ms | 8.2 KB | 0.305 ms | 1 |
| 50 interleaved branches | 10,000 | 56.108 ms | 1.042 ms | 18.5 KB | 0.744 ms | 50 |
| Repeated merges | 10,000 | 24.683 ms | 0.540 ms | 8.3 KB | 0.303 ms | 2 |

The largest continuation token was 2,049 bytes for the 50-branch case. Layout did not dominate any measured 100-row request. `bench_history` measured the integrated first page of a 10,000-commit linear Git fixture: Git 63.701 ms, parsing 0.271 ms, graph 0.348 ms, total Rust 64.429 ms, full-page JSON serialization 1.927 ms, and JSON payload 32,458 bytes. This run differs substantially from the earlier M4 machine/filesystem baseline; compare within a run before drawing scaling conclusions.

`node scripts/bench-graph-render.mjs` uses the production `GraphRowView` and history's virtual-window calculation at 100 positions across 10,000 loaded synthetic rows. At most 16 SVGs and 144 paths were rendered at a position; median server-side React render time was 1.756 ms, worst 14.783 ms including cold work. A synthetic 1,000-parent row emitted only 12 visible paths. This is a bounded-render diagnostic, not a browser frame-time or Tauri IPC measurement. Native/browser UI automation was unavailable in this run, so visible scrolling and dropped frames remain unmeasured.
## M8 local remote diagnostic, 2026-09-25

Temporary bare remote and two local clones exercise fetch, pull, and push without network variability. A representative Windows debug integration run measured fetch Git 255 ms plus branch refresh 40 ms, pull Git 350 ms plus HEAD/status/branch refresh 142 ms, and push Git 343 ms plus branch refresh 49 ms. The fetch test measured 397 ms wall time including configured-remote validation, scheduling, and polling. Fetch has one validation Git process, one fetch process, and one branch refresh process; pull uses four processes including three targeted refreshes; push uses two. These are diagnostic samples, not network throughput benchmarks or Tauri/WebView responsiveness measurements. The process runner bounds captured stdout to 64 KiB and stderr to 256 KiB. No change was made to the M4 history, M5 diff, M6 branch, or M7 graph performance baselines.

## M9 local refs and stash diagnostics, 2026-09-25

`cargo run --example bench_m9` creates a temporary repository, adds tag refs using one `git update-ref --stdin` process per size, and samples bulk tag/stash listing three times in a Windows debug build. Values below are the median-total sample. Fixture creation is excluded.

| Collection | Count | Git process | Rust parse | Total |
| --- | ---: | ---: | ---: | ---: |
| Tags | 20 | 41.320 ms | 0.037 ms | 41.369 ms |
| Tags | 200 | 89.607 ms | 0.288 ms | 89.905 ms |
| Tags | 1,000 | 180.374 ms | 0.879 ms | 181.266 ms |
| Stashes | 10 | 58.223 ms | 0.033 ms | 58.279 ms |

Both lists use one Git process, with no per-ref/stash subprocesses. Git/process/filesystem time dominates Rust parsing. These are local diagnostics with visible filesystem variation, not GUI frame times or Fork comparisons. Tag rows are virtualized; native WebView interaction remains unmeasured.

## M10 conflict diagnostics, 2026-09-25

A generated Windows debug fixture produced 11 text conflicts, including one 300 KiB file. `merge_branch` logged Git merge 110 ms and post-operation refresh 198 ms for that sample. A subsequent `get_repository_state` snapshot (status, merge marker, unmerged index metadata) took 104 ms. Full content was not loaded for the list; selecting the 300 KiB file returned semantic oversized flags under the 256 KiB per-side cap. These are single local diagnostics, not stable latency targets. There is one bulk `ls-files -u -z` process for conflicted index metadata, no Git process per rendered line; individual selected sides load on demand. Git/process/filesystem cost dominates parser work, but separate parsing timing was not recorded. Native WebView conflict-editor responsiveness remains unmeasured.

## M11 history-operation diagnostics, 2026-09-25

M11 adds no history or graph work to a repository-state refresh. Each history mutation runs one Git action under the existing session mutex and refreshes HEAD, branches, status, and operation markers once. An unmerged-index query runs only if status reports conflicts. Conflict blob content remains on demand. Hard reset adds bounded non-index and target-tree path-list queries to protect untracked and ignored collisions; those queries have 8 MiB output limits and may conservatively refuse very large repositories. Diagnostic timing is recorded in `docs/handoff.md`; local filesystem and process startup variation dominate short operations. No network or native WebView frame measurements were added.

## M12 partial-staging diagnostic, 2026-09-25

One Windows debug integration run of small temporary files logged approximately 40–60 ms for `git apply` and 110–170 ms for the targeted status and staged/unstaged file-diff refresh. This is a diagnostic range across individual tests, not a benchmark distribution. One semantic action applies all selected lines in its hunk with one Git apply process; it does not invoke Git per line. The existing 2 MiB raw patch and 20,000 parsed-line limits remain. Native WebView line-selection and scrolling times were not measured; the UI/UX phase should measure them before changing rendering strategy.
