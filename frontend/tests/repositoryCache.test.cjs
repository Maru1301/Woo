const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const ts = require("typescript");

const source = path.join(__dirname, "../src/app/repository-session/repositoryCache.ts");
const compiled = ts.transpileModule(fs.readFileSync(source, "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const loaded = new Module(source, module);
loaded.filename = source;
loaded.paths = module.paths;
loaded._compile(compiled, source);
const { RepositoryCache, RepositoryPrewarmQueue, repositoryCacheKey, sameIdentity, needsColdHistoryLoad, retainedCommitHash, refsChanged } = loaded.exports;

function entry(pathName, subject = "first") {
  const commit = { hash: subject, parentHashes: [], authorName: "A", authorEmail: "a@b", timestamp: "2026-01-01", subject, refs: [] };
  return {
    session: { repository: { path: pathName, branch: "main", head: { hash: subject } },
      status: { phase: "ready", data: { staged: [], unstaged: [], untracked: [], conflicted: [] } },
      branches: { phase: "ready", data: { branches: [] } }, operation: { kind: "none" }, conflicts: [] },
    history: { history: { commits: [commit], graphRows: [{ nodeLane: 0, laneCount: 1, incoming: false, continuations: [], parentLanes: [] }] }, pageLoaded: true,
      maxLanes: 1, cursor: "page-2", hasMore: true, selectedHash: subject, selectedSnapshot: commit,
      commitFiles: [{ path: "src/App.tsx", oldPath: null, kind: "modified" }], filesLoaded: true,
      selectedFile: { path: "src/App.tsx", oldPath: null, kind: "modified" }, selectedDiff: { hunks: [] },
      commitFilesMode: "tree", scrollTop: 544 },
    activeView: "history", localFilesMode: "list", selectedChange: null, localDiff: null,
    commitMessage: "draft", conflictVersion: 0, historyVersion: 1, historyRefreshVersion: 0, diffRefreshVersion: 0,
    tagRefreshVersion: 0, stashRefreshVersion: 0, refs: { branches: null, tags: null, stashes: null }, freshness: "fresh", generation: 0,
  };
}

test("warm repository switch restores one coherent history and presentation snapshot", () => {
  const cache = new RepositoryCache();
  cache.set("/woo", entry("/woo", "woo-head"));
  cache.set("/api", entry("/api", "api-head"));
  const warm = cache.get("/woo");
  assert.equal(warm.history.history.commits[0].hash, warm.history.selectedHash);
  assert.equal(warm.history.history.graphRows.length, warm.history.history.commits.length);
  assert.equal(warm.history.cursor, "page-2");
  assert.equal(warm.history.scrollTop, 544);
  assert.equal(warm.history.selectedFile.path, "src/App.tsx");
  assert.equal(warm.history.commitFilesMode, "tree");
  assert.equal(warm.commitMessage, "draft");
  assert.equal(needsColdHistoryLoad(warm.history), false);
  assert.equal(cache.get("/api").history.selectedHash, "api-head");
});

test("cold, warming, ready and stale states keep a stale snapshot displayable", () => {
  const cache = new RepositoryCache();
  assert.equal(cache.warmState("/repo"), "cold");
  assert.equal(cache.beginWarm("/repo"), true);
  assert.equal(cache.warmState("/repo"), "warming");
  cache.set("/repo", entry("/repo"));
  assert.equal(cache.warmState("/repo"), "ready");
  cache.markStale("/repo");
  assert.equal(cache.warmState("/repo"), "stale");
  assert.equal(cache.get("/repo").history.pageLoaded, true);
  assert.equal(cache.get("/repo").session.status.phase, "ready");
});

test("foreground loads first and a selected queued repository is promoted", () => {
  const queue = new RepositoryPrewarmQueue();
  queue.restore([{ id: "A", path: "/a" }, { id: "B", path: "/b" },
    { id: "C", path: "/c" }, { id: "D", path: "/d" }], "A");
  assert.equal(queue.nextBackground(), null, "background waits for A to become usable");
  assert.deepEqual(queue.promote("D"), { id: "D", path: "/d" });
  queue.releaseForeground();
  assert.deepEqual(queue.nextBackground(), { id: "B", path: "/b" });
  assert.deepEqual(queue.nextBackground(), { id: "C", path: "/c" });
  assert.equal(queue.nextBackground(), null);
});

test("an empty but loaded history remains a usable warm session", () => {
  const cache = new RepositoryCache();
  const saved = entry("/empty");
  saved.history.history = { commits: [], graphRows: [] };
  saved.history.pageLoaded = true;
  saved.history.cursor = null;
  saved.history.hasMore = false;
  cache.set("/empty", saved);
  assert.equal(cache.get("/empty").history.pageLoaded, true);
  assert.equal(cache.get("/empty").history.hasMore, false);
  assert.equal(needsColdHistoryLoad(cache.get("/empty").history), false);
  assert.equal(needsColdHistoryLoad(null), true);
});

test("stale cache remains visible until matching refresh finishes", () => {
  const cache = new RepositoryCache();
  cache.set("/woo", entry("/woo"));
  cache.markStale("/woo");
  assert.equal(cache.get("/woo").history.history.commits[0].subject, "first");
  const generation = cache.beginRefresh("/woo");
  assert.equal(cache.get("/woo").freshness, "refreshing");
  cache.finishRefresh("/woo", generation, (old) => ({ ...old, history: entry("/woo", "new").history, freshness: "fresh" }));
  assert.equal(cache.get("/woo").history.history.commits[0].subject, "new");
});

test("older refresh cannot replace a newer generation or another repository", () => {
  const cache = new RepositoryCache();
  cache.set("/woo", entry("/woo"));
  cache.set("/api", entry("/api", "api-head"));
  const first = cache.beginRefresh("/woo");
  const second = cache.beginRefresh("/woo");
  assert.equal(cache.finishRefresh("/woo", first, (old) => ({ ...old, history: entry("/woo", "old-result").history })), false);
  assert.equal(cache.finishRefresh("/woo", second, (old) => ({ ...old, history: entry("/woo", "new-result").history })), true);
  assert.equal(cache.get("/api").history.selectedHash, "api-head");
  assert.equal(cache.get("/woo").history.selectedHash, "new-result");
});

test("late asynchronous A response updates only A after B becomes active", async () => {
  const cache = new RepositoryCache();
  cache.set("/woo", entry("/woo"));
  cache.set("/api", entry("/api", "api-head"));
  const generation = cache.beginRefresh("/woo");
  let complete;
  const pending = new Promise((resolve) => { complete = resolve; });
  const task = pending.then(() => cache.finishRefresh("/woo", generation, (old) => ({ ...old, history: entry("/woo", "woo-updated").history })));
  const activePath = "/api";
  complete();
  await task;
  assert.equal(cache.get(activePath).history.selectedHash, "api-head");
  assert.equal(cache.get("/woo").history.selectedHash, "woo-updated");
});

test("workspace membership pruning retains a shared repository and evicts removed repositories", () => {
  const cache = new RepositoryCache(2);
  cache.set("/woo", entry("/woo"));
  cache.set("/api", entry("/api"));
  cache.get("/woo");
  cache.set("/docs", entry("/docs"));
  assert.equal(cache.get("/api"), undefined);
  cache.prune({ workspaces: [
    { repositories: [{ path: "/woo" }] },
    { repositories: [{ path: "/woo" }, { path: "/docs" }] },
  ] });
  assert.ok(cache.get("/woo"));
  assert.ok(cache.get("/docs"));
  cache.prune({ workspaces: [{ repositories: [{ path: "/docs" }] }] });
  assert.equal(cache.get("/woo"), undefined);
});

test("mounted views stay cached beyond the ordinary LRU limit until their tab is removed", () => {
  const cache = new RepositoryCache(2);
  for (const name of ["A", "B", "C"]) {
    cache.retainView(`/${name}`);
    cache.set(`/${name}`, entry(`/${name}`, `${name}-head`));
  }
  assert.equal(cache.get("/A").history.selectedHash, "A-head");
  assert.equal(cache.get("/B").history.selectedHash, "B-head");
  assert.equal(cache.get("/C").history.selectedHash, "C-head");
  cache.releaseView("/A");
  assert.equal(cache.get("/A"), undefined);
  assert.ok(cache.get("/B"));
  assert.ok(cache.get("/C"));
});

test("HEAD and branch identity determines whether history is stale", () => {
  const saved = entry("/woo").session.repository;
  assert.equal(sameIdentity(saved, { ...saved, sessionId: 99 }), true);
  assert.equal(sameIdentity(saved, { ...saved, head: { hash: "different" } }), false);
  assert.equal(sameIdentity(saved, { ...saved, branch: "feature" }), false);
});

test("selection survives a changed page only when its immutable commit is readable", () => {
  assert.equal(retainedCommitHash("deep", ["head"], "deep", true), "deep");
  assert.equal(retainedCommitHash("deep", ["head"], "deep", false), null);
  assert.equal(retainedCommitHash("head", ["head"], null, false), "head");
  assert.equal(retainedCommitHash("new-selection", ["head"], "old-selection", true), null);
});

test("fresh HEAD and refs avoid a history reload; changed or unknown refs require one", () => {
  const baseline = { branches: { branches: [{ name: "main", targetHash: "a" }] }, tags: { tags: [] }, stashes: { stashes: [] } };
  assert.equal(refsChanged(baseline, baseline), false);
  assert.equal(refsChanged(baseline, { ...baseline, tags: { tags: [{ name: "v1" }] } }), true);
  assert.equal(refsChanged({ ...baseline, stashes: null }, baseline), true);
});

test("Windows spelling differences share one canonical-path cache key", () => {
  assert.equal(repositoryCacheKey("C:/Work/Woo"), repositoryCacheKey("c:\\work\\woo"));
});
