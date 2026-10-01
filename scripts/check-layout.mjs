// `pnpm check:layout`: opens the app in a real browser (headless Chromium via playwright-core) in
// each screen state and checks that the page itself never scrolls. Yhtye fills the window with a
// fixed layout; only the panels' own scroll areas may scroll. jsdom computes no layout, so this
// is the check that catches an element that pokes out of the window (see docs/DEVELOPMENT.md).
//
// The page is `layout-harness.html` (src/test/layoutHarness.tsx): the real App on an in-memory
// core, with far more content than fits. It needs Vite only, not the Rust core. A free port is
// used for Vite (1420 / 1422 may belong to a running `pnpm dev:browser` or `tauri dev`).
//
//   YHTYE_CHROMIUM=/path/to/chromium   the browser (default: chromium / chrome found on PATH-like
//                                      locations, else Playwright's own download)
//   YHTYE_LAYOUT_URL=http://host:port  use a running dev server instead of starting one
//   YHTYE_LAYOUT_SHOTS=dir             also save a screenshot of every state there

import { spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CHROMIUM_CANDIDATES = ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome-stable", "/usr/bin/google-chrome"];
/**
 * Window sizes in CSS px: the least the panels fit in (their minimum widths add up to 1058px; the
 * window is zoomed to this much, e.g. 1440px at 135%), the smallest window the app has
 * (src-tauri/tauri.conf.json minWidth / minHeight), the default one (width / height), and a big one.
 */
const VIEWPORTS = [
  { width: 1060, height: 600 },
  { width: 1180, height: 700 },
  { width: 1440, height: 900 },
  { width: 1920, height: 1080 },
];
/** Interaction states are run in all but the biggest window. */
const INTERACTIVE_VIEWPORTS = VIEWPORTS.slice(0, 3);
const READY_TIMEOUT_MS = 30_000;
const STRUCTURE = [
  "html", "body", "#root", ".app", ".titlebar", ".banners", ".body", ".rail", ".sidebar", ".branches", ".workspace",
  ".conversation", ".right", ".tasks", ".bottom", ".statusbar", ".modal-backdrop", ".settings-modal",
];
/** Pop-ups that must lie inside the window and inside every box that clips them. */
const OVERLAYS = [".row-menu", ".project-popup", ".picker-popover", ".settings-modal"];

// ---- the checks, run inside the page ----------------------------------------------------------

/** Returns what is wrong with the page now: a list of readable messages (empty: fine). */
function inspectPage({ structure, overlays }) {
  const problems = [];
  const round = (n) => Math.round(n * 10) / 10;
  const doc = document.scrollingElement;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  if (doc.scrollHeight > vh) problems.push(`the page is ${doc.scrollHeight - vh}px taller than the window (scrollHeight ${doc.scrollHeight} > ${vh})`);
  if (doc.scrollWidth > vw) problems.push(`the page is ${doc.scrollWidth - vw}px wider than the window (scrollWidth ${doc.scrollWidth} > ${vw})`);
  if (window.scrollX !== 0 || window.scrollY !== 0) problems.push(`the page is scrolled (scrollX ${window.scrollX}, scrollY ${window.scrollY})`);

  // Scrolling it from code (what focus() and scrollIntoView() do) must not move anything either.
  window.scrollTo(1e6, 1e6);
  if (window.scrollX !== 0 || window.scrollY !== 0) problems.push(`the page can be scrolled from script (to ${window.scrollX}, ${window.scrollY})`);
  window.scrollTo(0, 0);

  const app = document.querySelector(".app");
  if (app) {
    const r = app.getBoundingClientRect();
    if (Math.abs(r.width - vw) > 0.5 || Math.abs(r.height - vh) > 0.5 || r.left !== 0 || r.top !== 0) {
      problems.push(`.app is ${round(r.width)}x${round(r.height)} at ${round(r.left)},${round(r.top)}, not the window ${vw}x${vh}`);
    }
  }

  // The frame of the layout never scrolls itself: nothing inside may be bigger than it and nothing
  // may have moved it (focus() scrolls even `overflow: hidden` boxes).
  for (const selector of structure) {
    for (const el of document.querySelectorAll(selector)) {
      if (el.scrollHeight > el.clientHeight + 1) problems.push(`${selector} has ${el.scrollHeight - el.clientHeight}px of content below its bottom edge (scrollHeight ${el.scrollHeight} > clientHeight ${el.clientHeight})`);
      if (el.scrollWidth > el.clientWidth + 1) problems.push(`${selector} has ${el.scrollWidth - el.clientWidth}px of content beyond its right edge (scrollWidth ${el.scrollWidth} > clientWidth ${el.clientWidth})`);
      if (el.scrollTop !== 0 || el.scrollLeft !== 0) problems.push(`${selector} is scrolled (scrollTop ${el.scrollTop}, scrollLeft ${el.scrollLeft})`);
    }
  }

  // Pop-ups: fully inside the window, and not cut off by a panel around them.
  for (const selector of overlays) {
    for (const el of document.querySelectorAll(selector)) {
      const style = getComputedStyle(el);
      if (style.visibility === "hidden" || style.display === "none") continue;
      const r = el.getBoundingClientRect();
      if (r.left < -0.5 || r.top < -0.5 || r.right > vw + 0.5 || r.bottom > vh + 0.5) {
        problems.push(`${selector} sticks out of the window (${round(r.left)},${round(r.top)} - ${round(r.right)},${round(r.bottom)} in ${vw}x${vh})`);
        continue;
      }
      if (style.position === "fixed") continue;
      for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
        const o = getComputedStyle(p);
        if (o.overflowX === "visible" && o.overflowY === "visible") continue;
        const c = p.getBoundingClientRect();
        if (r.left < c.left - 0.5 || r.top < c.top - 0.5 || r.right > c.right + 0.5 || r.bottom > c.bottom + 0.5) {
          problems.push(`${selector} is cut off by ${p.tagName.toLowerCase()}.${String(p.className).split(" ")[0]} (${round(r.right)},${round(r.bottom)} beyond ${round(c.right)},${round(c.bottom)})`);
          break;
        }
      }
    }
  }

  // Name the likely culprits: absolutely positioned elements below / to the right of the window
  // whose containing block is the page or the frame of the layout, so that nothing but the page
  // (or the frame) clips them or scrolls them away.
  if (problems.length > 0) {
    const frame = new Set(document.querySelectorAll(structure.join(",")));
    const containingBlock = (el) => {
      for (let p = el.parentElement; p; p = p.parentElement) {
        const s = getComputedStyle(p);
        if (s.position !== "static" || s.transform !== "none" || s.filter !== "none") return p;
      }
      return null;
    };
    const culprits = new Map();
    for (const el of document.querySelectorAll("body *")) {
      const position = getComputedStyle(el).position;
      if (position !== "absolute" && position !== "fixed") continue;
      const r = el.getBoundingClientRect();
      if (r.bottom <= vh + 1 && r.right <= vw + 1) continue;
      const cb = position === "fixed" ? null : containingBlock(el);
      if (cb && !frame.has(cb)) continue;
      const name = (e) => `${e.tagName.toLowerCase()}${typeof e.className === "string" && e.className ? `.${e.className.trim().split(/\s+/).join(".")}` : ""}`;
      const key = `${name(el)} (${position}, in ${cb ? name(cb) : "the page"})`;
      const seen = culprits.get(key) ?? { count: 0, bottom: 0, right: 0 };
      culprits.set(key, { count: seen.count + 1, bottom: Math.max(seen.bottom, Math.round(r.bottom)), right: Math.max(seen.right, Math.round(r.right)) });
    }
    for (const [key, c] of [...culprits].slice(0, 5)) problems.push(`  outside the window: ${c.count} x ${key}, down to y=${c.bottom}, x=${c.right}`);
  }
  return problems;
}

// ---- the screen states -------------------------------------------------------------------------

const click = (page, name, options = {}) => page.getByRole("button", { name, ...options }).first().click();

async function openProjectPicker(page) {
  await page.locator(".project-trigger").click();
  await page.locator(".project-popup").waitFor();
}

async function openRowMenu(page, which) {
  const more = page.locator(".chat-more");
  await more[which]().focus();
  await more[which]().click();
  await page.locator(".row-menu").waitFor();
}

/** Scrolls the sidebar's tree to the bottom, so that the last row's menu has no room below. */
const scrollTreeToEnd = (page) => page.locator(".branch-tree").evaluate((el) => void (el.scrollTop = el.scrollHeight));

async function wheelOver(page, selector, deltaY) {
  const box = await page.locator(selector).first().boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 12; i++) await page.mouse.wheel(0, deltaY);
  await page.waitForTimeout(150);
}

async function tabThrough(page, count) {
  const problems = [];
  await page.locator(".titlebar").evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  for (let i = 0; i < count; i++) {
    await page.keyboard.press("Tab");
    const found = await page.evaluate(inspectPage, { structure: STRUCTURE, overlays: OVERLAYS });
    if (found.length > 0) {
      const focused = await page.evaluate(() => `${document.activeElement?.tagName.toLowerCase()}.${String(document.activeElement?.className).split(" ")[0]}`);
      problems.push(...found.map((f) => `after Tab #${i + 1} (focus on ${focused}): ${f}`));
      break;
    }
  }
  return problems;
}

async function failSendThreeTimes(page) {
  await page.evaluate(() => {
    window.__yhtye.failCommand("send_user_message", "the project's orchestration has stopped, and this is a long message to see how the banner wraps");
  });
  const box = page.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
  for (const text of ["one", "two", "three"]) {
    await box.fill(text);
    await box.press("Enter");
    await page.waitForTimeout(100);
  }
  await page.evaluate(() => window.__yhtye.transport.setStatus({ state: "closed", reason: "bridge stopped", retryInMs: 1500 }));
  await page.locator(".banners .alert").first().waitFor();
}

/** { name, scenario, interactive, run(page) }: `run` brings the page into the state (default: as loaded). */
const STATES = [
  { name: "no project open", scenario: "empty" },
  { name: "the recorded run", scenario: "run" },
  { name: "long conversation, many tasks and branches", scenario: "long" },
  { name: "long: conversation scrolled to the end and wheeled on", scenario: "long", interactive: true, run: (page) => wheelOver(page, ".conversation .scroll", 4000) },
  { name: "long: conversation scrolled to the top and wheeled on", scenario: "long", interactive: true, run: (page) => wheelOver(page, ".conversation .scroll", -4000) },
  { name: "long: task list wheeled on", scenario: "long", interactive: true, run: (page) => wheelOver(page, ".tasks .scroll", 4000) },
  { name: "long: git panel wheeled on", scenario: "long", interactive: true, run: (page) => wheelOver(page, ".bottom .scroll", 4000) },
  { name: "long: branch tree wheeled on", scenario: "long", interactive: true, run: (page) => wheelOver(page, ".branch-tree", 4000) },
  { name: "long: project pull-down open", scenario: "long", interactive: true, run: openProjectPicker },
  { name: "long: project pull-down open, arrowed down to the last project", scenario: "long", interactive: true, run: async (page) => {
    await openProjectPicker(page);
    await page.keyboard.press("End");
    await page.waitForTimeout(100);
  } },
  { name: "long: chat menu on the first row", scenario: "long", interactive: true, run: (page) => openRowMenu(page, "first") },
  { name: "long: chat menu on the last row", scenario: "long", interactive: true, run: async (page) => { await scrollTreeToEnd(page); await openRowMenu(page, "last"); } },
  { name: "long: a task selected (agent output)", scenario: "long", interactive: true, run: async (page) => { await click(page, /^1 件目/, { exact: false }); await page.getByRole("region", { name: "agent output" }).waitFor(); } },
  { name: "long: 30 lines typed in the composer", scenario: "long", interactive: true, run: (page) => page.getByRole("textbox", { name: "オーケストレータへのメッセージ" }).fill(Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join("\n")) },
  { name: "long: error banners and a lost connection", scenario: "long", interactive: true, run: failSendThreeTimes },
  { name: "long: right panel and git panel dragged to their limits", scenario: "long", interactive: true, run: async (page) => {
    for (const [label, keys] of [["右パネルの幅", ["End", "Home"]], ["git パネルの高さ", ["Home", "End"]], ["サイドバーの幅", ["End", "Home"]]]) {
      await page.getByRole("separator", { name: label }).focus();
      for (const key of keys) await page.keyboard.press(`Shift+${key}`);
    }
  } },
  { name: "long: settings (agents)", scenario: "long", interactive: true, run: async (page) => { await click(page, "設定"); await page.getByRole("dialog", { name: "設定" }).waitFor(); } },
  { name: "long: settings, model pick-list open", scenario: "long", interactive: true, run: async (page) => {
    await click(page, "設定");
    await page.getByRole("dialog", { name: "設定" }).waitFor();
    // The project scope inherits the global settings (read-only); the global scope can be edited.
    await click(page, "全体", { exact: true });
    await page.locator(".picker-button:not(:disabled)").first().click();
    await page.locator(".picker-popover").waitFor();
  } },
  { name: "long: settings (secret env)", scenario: "long", interactive: true, run: async (page) => { await click(page, "設定"); await click(page, "秘密の環境変数"); } },
  { name: "long: every control reached with Tab", scenario: "long", interactive: true, tab: 80 },
  { name: "run: every control reached with Tab", scenario: "run", interactive: true, tab: 60 },
];

// ---- plumbing ----------------------------------------------------------------------------------

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function startVite() {
  const port = await freePort();
  const vite = path.join(root, "node_modules", ".bin", "vite");
  const child = spawn(vite, ["--host", "127.0.0.1", "--port", String(port), "--strictPort"], { cwd: root, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, TAURI_DEV_HOST: "" } });
  let output = "";
  child.stdout.on("data", (d) => (output += d));
  child.stderr.on("data", (d) => (output += d));
  const exited = new Promise((_, reject) => child.once("exit", (code) => reject(new Error(`vite exited (${code}):\n${output}`))));
  const url = `http://127.0.0.1:${port}`;
  const ready = (async () => {
    const deadline = Date.now() + READY_TIMEOUT_MS;
    while (Date.now() < deadline) {
      try {
        if ((await fetch(`${url}/layout-harness.html`)).ok) return;
      } catch {
        // not listening yet
      }
      await new Promise((r) => setTimeout(r, 200));
    }
    throw new Error(`vite did not come up on ${url}:\n${output}`);
  })();
  await Promise.race([ready, exited]);
  return { url, stop: () => child.kill("SIGTERM") };
}

function chromiumPath() {
  const fromEnv = process.env.YHTYE_CHROMIUM;
  if (fromEnv) {
    if (!existsSync(fromEnv)) throw new Error(`YHTYE_CHROMIUM=${fromEnv} does not exist`);
    return fromEnv;
  }
  return CHROMIUM_CANDIDATES.find(existsSync); // undefined: Playwright's own download, if any
}

async function openState(browser, baseUrl, state, viewport) {
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(`uncaught error: ${e.message}`));
  // Nothing outside the dev server (the web fonts of tokens.css): keeps the run offline and the same everywhere.
  await page.route((u) => !u.href.startsWith(baseUrl), (route) => route.abort());
  for (let attempt = 1; ; attempt++) {
    try {
      await page.goto(`${baseUrl}/layout-harness.html?scenario=${state.scenario}`);
      await page.waitForFunction(() => document.documentElement.dataset.ready, null, { timeout: READY_TIMEOUT_MS });
      break;
    } catch (e) {
      // Vite may reload the page once after it optimizes dependencies on its first run.
      if (attempt >= 3) throw e;
    }
  }
  return { context, page, errors };
}

async function checkState(browser, baseUrl, state, viewport, shots) {
  const { context, page, errors } = await openState(browser, baseUrl, state, viewport);
  try {
    await state.run?.(page);
    await page.waitForTimeout(200);
    const problems = state.tab ? await tabThrough(page, state.tab) : await page.evaluate(inspectPage, { structure: STRUCTURE, overlays: OVERLAYS });
    if (shots) await page.screenshot({ path: path.join(shots, `${state.name.replace(/[^\w]+/g, "-")}-${viewport.width}x${viewport.height}.png`) });
    return [...errors, ...problems];
  } catch (e) {
    return [...errors, `could not bring the page into this state: ${e instanceof Error ? e.message.split("\n")[0] : e}`];
  } finally {
    await context.close();
  }
}

async function main() {
  const external = process.env.YHTYE_LAYOUT_URL?.replace(/\/$/, "");
  const server = external ? { url: external, stop: () => {} } : await startVite();
  const shots = process.env.YHTYE_LAYOUT_SHOTS;
  if (shots) mkdirSync(shots, { recursive: true });
  const executablePath = chromiumPath();
  const browser = await chromium.launch({ executablePath, headless: true, args: ["--no-sandbox"] });
  let failed = 0;
  let checked = 0;
  try {
    // The first load warms Vite up (dependency optimization), so the real ones are fast and stable.
    await (await openState(browser, server.url, STATES[0], VIEWPORTS[0])).context.close();
    for (const state of STATES) {
      for (const viewport of state.interactive ? INTERACTIVE_VIEWPORTS : VIEWPORTS) {
        const problems = await checkState(browser, server.url, state, viewport, shots);
        checked++;
        const label = `${state.name} @ ${viewport.width}x${viewport.height}`;
        if (problems.length === 0) {
          console.log(`ok    ${label}`);
        } else {
          failed++;
          console.log(`FAIL  ${label}`);
          for (const p of problems) console.log(`        ${p}`);
        }
      }
    }
  } finally {
    await browser.close();
    server.stop();
  }
  console.log(failed === 0 ? `\nlayout: ${checked} states checked, the page never scrolls.` : `\nlayout: ${failed} of ${checked} states fail.`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(2);
});
