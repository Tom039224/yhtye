// `pnpm dev:browser [--vite-port N] [--bridge-port N] [--port-base N] [bridge args...]`: the React
// app in a plain browser against the real core (core-design.md §10). Builds and starts
// yhtye-dev-bridge (ws://127.0.0.1:1422/ws) and Vite (http://localhost:1420) with one shared
// random token, and stops both together (Ctrl+C, or when either exits).
// The ports can be changed (to run next to a `tauri dev` or another dev:browser), by the options
// above or by $YHTYE_VITE_PORT / $YHTYE_BRIDGE_PORT / $YHTYE_PORT_BASE (scripts/dev-browser-ports.mjs).
// Other arguments go to the bridge, e.g. `pnpm dev:browser --data-dir /tmp/y`.

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { bridgeArgs, PORT_HELP, resolvePorts } from "./dev-browser-ports.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const token = process.env.YHTYE_BRIDGE_TOKEN || randomBytes(16).toString("hex");

function fail(message) {
  console.error(`[dev:browser] ${message}`);
  process.exit(1);
}

/** Whether something listens on `port` (on IPv4 or IPv6 loopback: "localhost" may be either). */
async function portInUse(port) {
  for (const host of ["127.0.0.1", "::1"]) {
    const used = await new Promise((resolve) => {
      const server = net.createServer();
      server.once("error", (e) => resolve(e.code === "EADDRINUSE"));
      server.listen(port, host, () => server.close(() => resolve(false)));
    });
    if (used) return true;
  }
  return false;
}

let ports;
try {
  ports = resolvePorts(process.argv.slice(2), process.env);
} catch (e) {
  console.error(`[dev:browser] ${e.message}\nport options:\n${PORT_HELP}`);
  process.exit(2);
}
const { vitePort, bridgePort } = ports;
// Before the (long) build: both ports are needed, and Vite is strict about its own.
for (const [name, port] of [["Vite", vitePort], ["the bridge", bridgePort]]) {
  if (await portInUse(port)) {
    fail(`port ${port} (${name}) is already in use; is another dev:browser or \`tauri dev\` running? Pick other ports, e.g. \`pnpm dev:browser --port-base 15420\`.\n${PORT_HELP}`);
  }
}

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

console.error(`[dev:browser] app http://localhost:${vitePort}, bridge ws://127.0.0.1:${bridgePort}/ws`);
start("bridge", path.join(targetDir, "debug", "yhtye-dev-bridge"), bridgeArgs(ports), {
  YHTYE_BRIDGE_TOKEN: token,
});
// `--port` beats `server.port` of vite.config.ts (1420, which `tauri dev` needs); the page learns
// where the bridge is from VITE_YHTYE_BRIDGE_URL (src/api/create.ts).
start("vite", path.join(root, "node_modules", ".bin", "vite"), ["--port", String(vitePort), "--strictPort"], {
  VITE_YHTYE_BRIDGE_TOKEN: token,
  VITE_YHTYE_BRIDGE_URL: `ws://127.0.0.1:${bridgePort}/ws`,
});
