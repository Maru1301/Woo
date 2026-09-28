const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const ts = require("typescript");
const { JSDOM } = require("jsdom");

const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "http://localhost" });
global.window = dom.window;
global.document = dom.window.document;
global.navigator = dom.window.navigator;
global.IS_REACT_ACT_ENVIRONMENT = true;

const React = require("react");
const { createRoot } = require("react-dom/client");
const source = path.join(__dirname, "../src/app/repository-session/RepositoryView.tsx");
const compiled = ts.transpileModule(fs.readFileSync(source, "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX },
}).outputText;
const loaded = new Module(source, module);
loaded.filename = source;
loaded.paths = module.paths;
loaded._compile(compiled, source);
const { RepositoryView, useRepositoryViewActive, useRepositoryViewId } = loaded.exports;
const e = React.createElement;

test("A to B to A to C to B to A retains mounted component and DOM identity, selection and scroll", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const mounts = { A: 0, B: 0, C: 0 };
  const unmounts = { A: 0, B: 0, C: 0 };
  function Probe({ repo }) {
    const active = useRepositoryViewActive();
    const [selected, setSelected] = React.useState("first");
    React.useEffect(() => { mounts[repo]++; return () => { unmounts[repo]++; }; }, [repo]);
    return e("div", { "data-repo": repo },
      e("button", { disabled: !active, onClick: () => setSelected("second") }, selected),
      e("div", { className: "scroll", style: { overflow: "auto", height: 20 } }, e("div", { style: { height: 300 } }, "content")));
  }
  function frame(active) {
    return e(React.Fragment, null, ...["A", "B", "C"].map((repo) => e(RepositoryView,
      { key: repo, active: active === repo, ready: true, className: "repo-view" },
      active === repo ? e(Probe, { repo }) : null)));
  }
  await React.act(async () => root.render(frame("A")));
  const a = host.querySelector('[data-repo="A"]');
  await React.act(async () => a.querySelector("button").click());
  a.querySelector(".scroll").scrollTop = 77;
  await React.act(async () => root.render(frame("B")));
  const b = host.querySelector('[data-repo="B"]');
  assert.equal(host.querySelector('[data-repo="A"]'), a);
  assert.equal(a.closest(".repo-view").hidden, true);
  assert.equal(a.querySelector("button").disabled, true);
  assert.equal(b.querySelector("button").disabled, false);
  await React.act(async () => root.render(frame("A")));
  await React.act(async () => root.render(frame("C")));
  await React.act(async () => root.render(frame("B")));
  await React.act(async () => root.render(frame("A")));
  assert.equal(host.querySelector('[data-repo="A"]'), a);
  assert.equal(a.querySelector("button").textContent, "second");
  assert.equal(a.querySelector(".scroll").scrollTop, 77);
  assert.deepEqual(mounts, { A: 1, B: 1, C: 1 });
  assert.deepEqual(unmounts, { A: 0, B: 0, C: 0 });
  await React.act(async () => root.unmount());
  host.remove();
});

test("visible toolbar uses its own repository state and closing one view unmounts only that view", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const unmounts = [];
  function Toolbar({ repo, canPush }) {
    const active = useRepositoryViewActive();
    React.useEffect(() => () => unmounts.push(repo), [repo]);
    return e("button", { "data-toolbar": repo, disabled: !active || !canPush }, "Push");
  }
  function frame(active, paths = ["A", "B"], ready = true) {
    return e(React.Fragment, null, ...paths.map((repo) => e(RepositoryView,
      { key: repo, active: active === repo, ready, className: "remote-view" },
      active === repo && ready ? e(Toolbar, { repo, canPush: repo === "A" }) : null)));
  }
  await React.act(async () => root.render(frame("A")));
  assert.equal(host.querySelector('[data-toolbar="A"]').disabled, false);
  await React.act(async () => root.render(frame("B")));
  assert.equal(host.querySelector('[data-toolbar="B"]').disabled, true);
  await React.act(async () => root.render(frame("A", ["A", "B"], false)));
  assert.equal(host.querySelector('[data-toolbar="A"]').disabled, true, "a genuinely cold view has no usable controls");
  await React.act(async () => root.render(frame("A")));
  assert.equal(host.querySelector('[data-toolbar="A"]').disabled, false);
  await React.act(async () => root.render(frame("B", ["B"])));
  assert.deepEqual(unmounts, ["A"]);
  assert.ok(host.querySelector('[data-toolbar="B"]'));
  await React.act(async () => root.unmount());
  host.remove();
});

test("rapid selection gives the visible action its repository ID immediately", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const targets = [];
  function Action() {
    const repositoryId = useRepositoryViewId();
    const active = useRepositoryViewActive();
    return e("button", { disabled: !active, onClick: () => targets.push(repositoryId) }, "Push");
  }
  function frame(selected) {
    return e(React.Fragment, null, ...["A", "B", "C"].map((id) => e(RepositoryView,
      { key: id, repositoryId: id, active: selected === id, ready: true, className: "remote-view" },
      selected === id ? e(Action) : null)));
  }
  await React.act(async () => root.render(frame("A")));
  await React.act(async () => root.render(frame("B")));
  assert.equal(host.querySelector('.remote-view:not([hidden]) button').disabled, false);
  await React.act(async () => host.querySelector('.remote-view:not([hidden]) button').click());
  await React.act(async () => root.render(frame("A")));
  await React.act(async () => root.render(frame("B")));
  await React.act(async () => root.render(frame("C")));
  await React.act(async () => host.querySelector('.remote-view:not([hidden]) button').click());
  assert.deepEqual(targets, ["B", "C"]);
  await React.act(async () => root.unmount());
  host.remove();
});

test("toolbar and repository content switch from the same snapshot in one frame", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const snapshots = {
    A: { branch: "main", changes: 1, canPush: false },
    B: { branch: "feature/b", changes: 3, canPush: true },
  };
  function frame(selected) {
    return e(React.Fragment, null, ...["A", "B"].map((id) => e(RepositoryView,
      { key: id, repositoryId: id, active: selected === id, ready: true, className: "repo-view" },
      selected === id ? e("section", { "data-repository": id },
        e("span", { "data-branch": id }, snapshots[id].branch),
        e("span", { "data-status": id }, `${snapshots[id].changes} changes`),
        e("button", { disabled: !snapshots[id].canPush }, "Push")) : null)));
  }
  await React.act(async () => root.render(frame("A")));
  await React.act(async () => root.render(frame("B")));
  const visible = host.querySelector('.repo-view:not([hidden])');
  assert.equal(visible.querySelector('[data-branch]').textContent, "feature/b");
  assert.equal(visible.querySelector('[data-status]').textContent, "3 changes");
  assert.equal(visible.querySelector('button').disabled, false);
  await React.act(async () => root.unmount());
  host.remove();
});
