export const DEFAULT_VITE_PORT: number;
export const DEFAULT_BRIDGE_PORT: number;
export const PORT_HELP: string;
export function resolvePorts(
  argv: string[],
  env: Record<string, string | undefined>,
): { vitePort: number; bridgePort: number; rest: string[] };
export function bridgeArgs(ports: { vitePort: number; bridgePort: number; rest: string[] }): string[];
