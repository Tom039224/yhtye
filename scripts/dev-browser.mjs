// `pnpm dev:browser [bridge args...]`: the React app in a plain browser against
// the real core (core-design.md §10). Builds and starts yhtye-dev-bridge
// (ws://127.0.0.1:1422/ws) and Vite (http://localhost:1420) with one shared
// random token, and stops both together (Ctrl+C, or when either exits).
// Extra arguments go to the bridge, e.g. `pnpm dev:browser --data-dir /tmp/y`.

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const token = process.env.YHTYE_BRIDGE_TOKEN || randomBytes(16).toString("hex");

const build = spawnSync("cargo", ["build", "-p", "yhtye-dev-bridge"], { cwd: root, stdio: "inherit" });
if (build.status !== 0) process.exit(build.status ?? 1);

const targetDir = process.env.CARGO_TARGET_DIR || path.join(root, "target");
const children = [];
let stopping = false;

function start(name, command, args, env) {
  const child = spawn(command, args, { cwd: root, stdio: "inherit", env: { ...process.env, ...env } });
  child.on("exit", (code, signal) => {
    console.error(`[dev:browser] ${name} exited (${signal ?? code})`);
    stop(code ?? 0);
  });
  children.push(child);
  return child;
}

function stop(code) {
  if (stopping) return;
  stopping = true;
  for (const c of children) {
    if (c.exitCode === null && c.signalCode === null) c.kill("SIGTERM");
  }
  // The bridge shuts its agents down before exiting; give it time.
  const deadline = setTimeout(() => {
    for (const c of children) if (c.exitCode === null && c.signalCode === null) c.kill("SIGKILL");
  }, 20_000);
  deadline.unref();
  Promise.all(children.map((c) => (c.exitCode !== null || c.signalCode !== null ? null : new Promise((r) => c.on("exit", r))))).then(
    () => process.exit(code),
  );
}

process.on("SIGINT", () => stop(0));
process.on("SIGTERM", () => stop(0));

start("bridge", path.join(targetDir, "debug", "yhtye-dev-bridge"), process.argv.slice(2), {
  YHTYE_BRIDGE_TOKEN: token,
});
start("vite", path.join(root, "node_modules", ".bin", "vite"), [], { VITE_YHTYE_BRIDGE_TOKEN: token });
