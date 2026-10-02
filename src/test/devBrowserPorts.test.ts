// The port options of `pnpm dev:browser` (scripts/dev-browser-ports.mjs).

import { describe, expect, it } from "vitest";

import { bridgeArgs, resolvePorts } from "../../scripts/dev-browser-ports.mjs";

describe("resolvePorts", () => {
  it("uses 1420 and 1422 when nothing is given", () => {
    expect(resolvePorts([], {})).toEqual({ vitePort: 1420, bridgePort: 1422, rest: [] });
  });

  it("takes --vite-port and --bridge-port, in either spelling", () => {
    expect(resolvePorts(["--vite-port", "15420", "--bridge-port=15422"], {})).toMatchObject({ vitePort: 15420, bridgePort: 15422 });
  });

  it("puts the bridge on --port-base + 2", () => {
    expect(resolvePorts(["--port-base", "15420"], {})).toMatchObject({ vitePort: 15420, bridgePort: 15422 });
  });

  it("lets the single ports win over --port-base", () => {
    expect(resolvePorts(["--port-base", "15420", "--bridge-port", "9000"], {})).toMatchObject({ vitePort: 15420, bridgePort: 9000 });
  });

  it("reads the environment, and lets the command line win", () => {
    const env = { YHTYE_VITE_PORT: "16420", YHTYE_BRIDGE_PORT: "16422" };
    expect(resolvePorts([], env)).toMatchObject({ vitePort: 16420, bridgePort: 16422 });
    expect(resolvePorts(["--vite-port", "17420"], env)).toMatchObject({ vitePort: 17420, bridgePort: 16422 });
    expect(resolvePorts([], { YHTYE_PORT_BASE: "18000" })).toMatchObject({ vitePort: 18000, bridgePort: 18002 });
  });

  it("ignores an empty environment variable", () => {
    expect(resolvePorts([], { YHTYE_VITE_PORT: "", YHTYE_BRIDGE_PORT: "" })).toMatchObject({ vitePort: 1420, bridgePort: 1422 });
  });

  it("reads the bridge's own --port as the bridge port", () => {
    expect(resolvePorts(["--port", "1500"], {})).toMatchObject({ vitePort: 1420, bridgePort: 1500, rest: [] });
  });

  it("passes everything else on to the bridge, in order", () => {
    const { rest } = resolvePorts(["--data-dir", "/tmp/y", "--vite-port", "15420", "--model", "sonnet"], {});
    expect(rest).toEqual(["--data-dir", "/tmp/y", "--model", "sonnet"]);
  });

  it.each([
    [["--vite-port", "abc"], /--vite-port: "abc" is not a port number/],
    [["--bridge-port", "0"], /not a port number/],
    [["--port-base=70000"], /not a port number/],
    [["--vite-port", "1e3"], /not a port number/],
    [["--vite-port"], /--vite-port needs a value/],
    [["--port-base", "65534"], /above 65535/],
    [["--vite-port", "2000", "--bridge-port", "2000"], /cannot both use port 2000/],
  ])("rejects %j", (argv, message) => {
    expect(() => resolvePorts(argv, {})).toThrow(message);
  });

  it("names the environment variable that is wrong", () => {
    expect(() => resolvePorts([], { YHTYE_BRIDGE_PORT: "x" })).toThrow(/\$YHTYE_BRIDGE_PORT: "x"/);
  });
});

describe("bridgeArgs", () => {
  it("passes the port and no extra origin for the default Vite port", () => {
    expect(bridgeArgs({ vitePort: 1420, bridgePort: 1422, rest: ["--data-dir", "/tmp/y"] })).toEqual(["--port", "1422", "--data-dir", "/tmp/y"]);
  });

  it("allows the page's origin for another Vite port", () => {
    expect(bridgeArgs({ vitePort: 15420, bridgePort: 15422, rest: [] })).toEqual([
      "--port", "15422",
      "--allow-origin", "http://localhost:15420",
      "--allow-origin", "http://127.0.0.1:15420",
    ]);
  });
});
