import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { delimiter, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const env = { ...process.env };
const cargo = process.platform === "win32" ? "cargo.exe" : "cargo";
const localCargoHome = join(root, ".tools", "cargo");
const localRustupHome = join(root, ".tools", "rustup");
const localCargo = join(localCargoHome, "bin", cargo);

if (spawnSync(cargo, ["--version"], { env, stdio: "ignore" }).status !== 0) {
  if (!existsSync(localCargo) || !existsSync(localRustupHome)) {
    console.error("Cargo is not on PATH. Install Rust from https://rustup.rs/ or make cargo available in this shell.");
    process.exit(1);
  }
  env.CARGO_HOME = localCargoHome;
  env.RUSTUP_HOME = localRustupHome;
  const pathKey = Object.keys(env).find((key) => key.toLowerCase() === "path") ?? "PATH";
  env[pathKey] = `${join(localCargoHome, "bin")}${delimiter}${env[pathKey] ?? ""}`;
}

const tauri = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const child = spawn(process.execPath, [tauri, ...process.argv.slice(2)], {
  cwd: root,
  env,
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(`Could not start Tauri: ${error.message}`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
