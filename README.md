# Woo

Woo is an independent desktop Git client built with Tauri 2, Rust, React, and TypeScript. It opens a local working-tree repository, supports staging and commits, browses paged commit history and its commit graph, displays on-demand text diffs, manages local branches with safe checkout, and runs fetch, pull, and push through system Git. Remote-tracking branches are displayed from local refs and update after fetch.

## Development

Prerequisites: system Git, Rust (stable MSVC toolchain on Windows), Node.js 20.19+ or 22.12+, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev
```

On Windows, the npm Tauri command uses a system Cargo when available. If Cargo is not on PATH, it uses the Rust toolchain already present in this workspace's ignored `.tools` directory. A fresh checkout without either toolchain needs a Rust installation.

Checks:

```sh
npm run build
cd src-tauri && cargo test
```

See [architecture](docs/architecture/overview.md), [performance plan](docs/performance/benchmark-plan.md), and [handoff](docs/handoff.md).
