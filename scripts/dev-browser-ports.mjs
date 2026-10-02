// The ports of `pnpm dev:browser` (scripts/dev-browser.mjs): which ones to use, and what to hand to
// the bridge. Kept apart from the script so that it can be tested (src/test/devBrowserPorts.test.ts).

export const DEFAULT_VITE_PORT = 1420;
export const DEFAULT_BRIDGE_PORT = 1422;
/** Vite 1420, its HMR 1421 (under `TAURI_DEV_HOST`), the bridge 1422: `--port-base N` keeps that spacing. */
const BRIDGE_OFFSET = DEFAULT_BRIDGE_PORT - DEFAULT_VITE_PORT;

export const PORT_HELP = [
  "  --vite-port N     Vite's port (default 1420; or $YHTYE_VITE_PORT)",
  "  --bridge-port N   the bridge's port (default 1422; or $YHTYE_BRIDGE_PORT); the bridge's own --port means the same",
  "  --port-base N     Vite on N, the bridge on N+2 (or $YHTYE_PORT_BASE); the two above win over it",
].join("\n");

function parsePort(text, source) {
  if (!/^\d+$/.test(text) || Number(text) < 1 || Number(text) > 65535) {
    throw new Error(`${source}: "${text}" is not a port number (1-65535)`);
  }
  return Number(text);
}

/** The value of `--flag value` / `--flag=value` at argv[i]: [value, index of the last argument used]. */
function flagValue(argv, i, flag) {
  const arg = argv[i];
  if (arg.startsWith(`${flag}=`)) return [arg.slice(flag.length + 1), i];
  if (i + 1 >= argv.length) throw new Error(`${flag} needs a value`);
  return [argv[i + 1], i + 1];
}

const FLAGS = { "--vite-port": "vite", "--bridge-port": "bridge", "--port": "bridge", "--port-base": "base" };

/**
 * Ports from the command line (they win) and the environment; everything the script does not
 * consume is returned as `rest`, for the bridge.
 *
 * @param {string[]} argv
 * @param {Record<string, string | undefined>} env
 * @returns {{ vitePort: number, bridgePort: number, rest: string[] }}
 * @throws {Error} with a message fit to print, for a bad value or clashing ports
 */
export function resolvePorts(argv, env) {
  const cli = {};
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const flag = Object.keys(FLAGS).find((f) => argv[i] === f || argv[i].startsWith(`${f}=`));
    if (!flag) {
      rest.push(argv[i]);
      continue;
    }
    const [value, last] = flagValue(argv, i, flag);
    cli[FLAGS[flag]] = parsePort(value, flag);
    i = last;
  }

  const fromEnv = (name) => (env[name] ? parsePort(env[name], `$${name}`) : undefined);
  const base = cli.base ?? fromEnv("YHTYE_PORT_BASE");
  const vitePort = cli.vite ?? fromEnv("YHTYE_VITE_PORT") ?? base ?? DEFAULT_VITE_PORT;
  const bridgePort = cli.bridge ?? fromEnv("YHTYE_BRIDGE_PORT") ?? (base === undefined ? DEFAULT_BRIDGE_PORT : base + BRIDGE_OFFSET);
  if (bridgePort > 65535) throw new Error(`the bridge would need port ${bridgePort} (--port-base + ${BRIDGE_OFFSET}), which is above 65535`);
  if (vitePort === bridgePort) throw new Error(`Vite and the bridge cannot both use port ${vitePort}`);
  return { vitePort, bridgePort, rest };
}

/**
 * The arguments for the bridge: its port, and (for a Vite port other than 1420) the page's origin,
 * which the bridge only accepts connections from.
 */
export function bridgeArgs({ vitePort, bridgePort, rest }) {
  const origins = vitePort === DEFAULT_VITE_PORT ? [] : [`http://localhost:${vitePort}`, `http://127.0.0.1:${vitePort}`].flatMap((o) => ["--allow-origin", o]);
  return ["--port", String(bridgePort), ...origins, ...rest];
}
