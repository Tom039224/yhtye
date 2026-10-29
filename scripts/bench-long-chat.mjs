// `pnpm bench:long-chat`: measures how a long chat makes the UI freeze (see docs/DEVELOPMENT.md).
// Opens `layout-harness.html?scenario=long&exchanges=<n>&mix=real` (the real App on an in-memory core,
// src/test/layoutHarness.tsx, src/test/benchChat.ts) in a real browser and measures, for each
// conversation length n:
//   idle     rAF intervals, blocks of the main thread and its CPU time over a quiet stretch
//   input    keydown -> next frame while typing and while an arrow key auto-repeats, and how long
//            the caret keeps moving after the key was let go ("runaway")
//   stream   the same while message chunks of the shown chat / of another task arrive every 20 ms
//   commit   the longest block of the main thread caused by one durable event (long answer, help, task result)
// each with the CSS animations running and (--anim both) with them switched off. WebKit (Playwright's
// build, the closest to the WebKitGTK of the app) is required; Chromium is optional.
//
//   --n 160,1000,3000        conversation lengths (round trips)
//   --browsers webkit[,chromium]   (chromium: YHTYE_CHROMIUM=/path or a system one, as in check-layout)
//   --mode prod|dev          production build of the harness (default; what the app ships) or Vite dev
//   --anim on|off|both       idle is always measured both ways; the rest with the animations running (default), stopped, or both (twice the time)
//   --sections idle,input,stream,commit   (stream = stream-shown + stream-other, which can be run apart)
//   --chars 30  --repeats 50   keys typed / arrow keydowns per burst (fewer when one key takes seconds: n = 3000)
//   --idle-ms 5000  --stream-ms 15000  --reps 3  --out bench-results  --url http://host:port (a running dev server)

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { chromium, webkit } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CHROMIUM_CANDIDATES = ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome-stable", "/usr/bin/google-chrome"];
const VIEWPORT = { width: 1440, height: 900 };
const READY_TIMEOUT_MS = 600_000;
const KEY_INTERVAL_MS = 20;
let TYPED_CHARS = 30;
let ARROW_REPEATS = 50;
const DRAIN_TIMEOUT_MS = 90_000;
const COMPOSER = 'textarea[aria-label="オーケストレータへのメッセージ"]';
const NO_ANIMATION_CSS = "*{animation:none!important;transition:none!important}";

/** Lets a style change be applied (on a big page the first frame after it recalculates every element) before a window opens. */
async function settleStyle(page) {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0)))));
  await sleep(1000);
}

function parseArgs(argv) {
  const opts = { n: [160, 1000, 3000], browsers: ["webkit"], mode: "prod", anim: "on", sections: ["idle", "input", "stream", "commit"], idleMs: 5000, streamMs: 15000, reps: 3, out: path.join(root, "bench-results"), url: null };
  const list = (v) => v.split(",").map((s) => s.trim()).filter(Boolean);
  for (let i = 0; i < argv.length; i++) {
    const [flag, inline] = argv[i].split("=");
    const value = () => inline ?? argv[++i];
    switch (flag) {
      case "--n": opts.n = list(value()).map(Number); break;
      case "--browsers": opts.browsers = list(value()); break;
      case "--mode": opts.mode = value(); break;
      case "--anim": opts.anim = value(); break;
      case "--sections": opts.sections = list(value()); break;
      case "--idle-ms": opts.idleMs = Number(value()); break;
      case "--stream-ms": opts.streamMs = Number(value()); break;
      case "--reps": opts.reps = Number(value()); break;
      case "--chars": TYPED_CHARS = Number(value()); break;
      case "--repeats": ARROW_REPEATS = Number(value()); break;
      case "--out": opts.out = path.resolve(value()); break;
      case "--url": opts.url = value().replace(/\/$/, ""); break;
      default: throw new Error(`unknown argument ${argv[i]} (see the header of scripts/bench-long-chat.mjs)`);
    }
  }
  if (opts.n.some((n) => !Number.isInteger(n) || n < 1)) throw new Error("--n: whole numbers from 1");
  if (!["prod", "dev"].includes(opts.mode)) throw new Error("--mode: prod or dev");
  if (!["both", "on", "off"].includes(opts.anim)) throw new Error("--anim: both, on or off");
  if (!opts.browsers.includes("webkit")) throw new Error("--browsers: WebKit is required (the app runs on WebKitGTK); add chromium for comparison");
  return opts;
}

// ---- numbers ----------------------------------------------------------------------------------

const round = (x, d = 1) => (x === null || Number.isNaN(x) ? null : Math.round(x * 10 ** d) / 10 ** d);
/** Nearest-rank percentile of a list (null when empty). */
function percentile(list, p) {
  if (list.length === 0) return null;
  const sorted = [...list].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}
const spread = (list) => ({ n: list.length, p50: round(percentile(list, 50)), p95: round(percentile(list, 95)), max: round(list.length ? Math.max(...list) : null) });
const fmt = (x) => (x === null || x === undefined ? "-" : String(x));
const triple = (s) => (s.n === 0 ? "-" : `${fmt(s.p50)}/${fmt(s.p95)}/${fmt(s.max)}`);

// ---- main-thread CPU of the web process (Linux: /proc) ---------------------------------------

const CLK_TCK = 100;
function procPids() {
  try {
    return readdirSync("/proc").filter((d) => /^\d+$/.test(d)).map(Number);
  } catch {
    return [];
  }
}
function cmdlineOf(pid) {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, "utf8").replaceAll("\0", " ");
  } catch {
    return "";
  }
}
/** utime + stime in ticks of a task file (`/proc/<pid>/stat`, `/proc/<pid>/task/<tid>/stat`). */
function ticksOf(file) {
  try {
    const stat = readFileSync(file, "utf8");
    const f = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    return Number(f[11]) + Number(f[12]);
  } catch {
    return null;
  }
}
/** The web / renderer processes that were not there before the browser started. */
function webProcesses(kind, before) {
  const match = kind === "webkit" ? /WPEWebProcess|WebKitWebProcess/ : /--type=renderer/;
  return procPids().filter((pid) => !before.has(pid) && match.test(cmdlineOf(pid)));
}
/** The busiest of them: the page's own. */
function pageProcess(kind, before) {
  let best = null;
  for (const pid of webProcesses(kind, before)) {
    const t = ticksOf(`/proc/${pid}/stat`) ?? 0;
    if (!best || t > best.t) best = { pid, t };
  }
  return best?.pid ?? null;
}
function cpuSample(pid) {
  if (pid === null || process.platform !== "linux") return null;
  return { at: performance.now(), main: ticksOf(`/proc/${pid}/task/${pid}/stat`), all: ticksOf(`/proc/${pid}/stat`) };
}
/** CPU% of the main thread and of the whole process between two samples. */
function cpuBetween(a, b) {
  if (!a || !b || a.main === null || b.main === null) return { mainPct: null, processPct: null };
  const secs = (b.at - a.at) / 1000;
  return { mainPct: round(((b.main - a.main) / CLK_TCK / secs) * 100, 0), processPct: round(((b.all - a.all) / CLK_TCK / secs) * 100, 0) };
}

// ---- the page side -----------------------------------------------------------------------------

/** Runs in the page once: the timer probe and the recorders the measurements read. */
function installBench() {
  const B = (window.__bench = {});
  const epoch = () => performance.timeOrigin + performance.now();
  B.epoch = epoch;
  // A timer chain that never asks for a pause: its gaps are the main thread's blocks.
  B.blocks = [];
  let last = performance.now();
  const tick = () => {
    const now = performance.now();
    if (now - last > 16) B.blocks.push([performance.timeOrigin + now, now - last]);
    last = now;
    setTimeout(tick, 0);
  };
  setTimeout(tick, 0);
  // Frames (rAF callbacks) while a window is open.
  let frames = null;
  let raf = 0;
  const frame = () => {
    if (frames) frames.push(epoch());
    raf = requestAnimationFrame(frame);
  };
  B.begin = () => {
    frames = [];
    B.blockFrom = B.blocks.length;
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(frame);
    return epoch();
  };
  B.end = () => {
    const out = { frames, blocks: B.blocks.slice(B.blockFrom).map((b) => b[1]), animations: document.getAnimations().length };
    frames = null;
    cancelAnimationFrame(raf);
    return out;
  };
  // Keys: when each reached the page, the next frame after it, and the first task after that frame.
  B.keys = [];
  B.keyups = [];
  B.clearKeys = () => {
    B.keys = [];
    B.keyups = [];
  };
  B.watch = (selector) => {
    if (B.watching) return;
    B.watching = true;
    const field = () => document.querySelector(selector);
    window.addEventListener("keydown", (e) => {
      const rec = { key: e.key, handledAt: epoch(), caret: field()?.selectionStart ?? null, raf: 0, after: 0 };
      B.keys.push(rec);
      requestAnimationFrame(() => {
        rec.raf = epoch();
        setTimeout(() => (rec.after = epoch()), 0);
      });
    }, true);
    window.addEventListener("keyup", (e) => B.keyups.push({ key: e.key, handledAt: epoch(), caret: field()?.selectionStart ?? null }), true);
  };
  B.keyState = () => ({ downs: B.keys.length, done: B.keys.filter((k) => k.after).length, ups: B.keyups.length });
  // One durable event: the longest block it causes, and how long until the frame after it.
  B.measureInject = async (kind) => {
    await new Promise((r) => setTimeout(r, 400));
    const from = B.blocks.length;
    const t0 = performance.now();
    window.__yhtye.bench.inject(kind);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0))));
    const paintMs = performance.now() - t0;
    await new Promise((r) => setTimeout(r, 600));
    const gaps = B.blocks.slice(from).map((b) => b[1]);
    return { paintMs, maxBlockMs: gaps.length ? Math.max(...gaps) : 0 };
  };
}

// ---- driving the keys ---------------------------------------------------------------------------

/** The offset of the page's clock from this process' (ms to subtract from the page's epoch times). */
async function clockOffset(page) {
  let best = null;
  for (let i = 0; i < 15; i++) {
    const a = Date.now();
    const p = await page.evaluate(() => window.__bench.epoch());
    const b = Date.now();
    if (!best || b - a < best.rtt) best = { rtt: b - a, offset: p - (a + b) / 2 };
  }
  return best.offset;
}

/**
 * Sends the steps at their times without waiting for any to be handled, as the operating system's key
 * repeat does: a page that cannot keep up gets a queue, not a slower typist.
 */
async function sendKeys(page, steps) {
  const start = performance.now();
  const sends = [];
  const pending = [];
  for (const step of steps) {
    const wait = step.at - (performance.now() - start);
    if (wait > 1) await sleep(wait);
    sends.push({ ...step, sentAt: Date.now() });
    pending.push(page.keyboard[step.action](step.key).catch(() => undefined));
  }
  return { sends, settled: Promise.allSettled(pending) };
}

async function waitForKeys(page, downs, ups) {
  const deadline = performance.now() + DRAIN_TIMEOUT_MS;
  for (;;) {
    const s = await page.evaluate(() => window.__bench.keyState());
    if (s.done >= downs && s.ups >= ups) return { drained: true };
    if (performance.now() > deadline) return { drained: false, ...s };
    await sleep(50);
  }
}

/** Latencies (ms) of the keydowns from sent to: reaching the page, the next frame, the first task after it. */
function keyLatencies(sends, keys, offset) {
  const downs = sends.filter((s) => s.action !== "up");
  const pairs = downs.slice(0, keys.length).map((s, i) => ({ s, k: keys[i] }));
  const since = (page, sent) => Math.max(0, page - offset - sent);
  return {
    handler: spread(pairs.map(({ s, k }) => since(k.handledAt, s.sentAt))),
    raf: spread(pairs.filter(({ k }) => k.raf).map(({ s, k }) => since(k.raf, s.sentAt))),
    frame: spread(pairs.filter(({ k }) => k.after).map(({ s, k }) => since(k.after, s.sentAt))),
    lost: downs.length - keys.length,
    // Main-thread time one key costs, whatever the pace they come at: first to last handled.
    perKeyMs: keys.length > 1 && keys.every((k) => k.after || k.raf) ? round(((keys[keys.length - 1].after || keys[keys.length - 1].raf) - keys[0].handledAt) / (keys.length - 1)) : null,
  };
}

async function typeBurst(page, offset) {
  await page.evaluate(() => window.__bench.clearKeys());
  const letters = "abcdefghijklmnopqrstuvwxyz";
  const steps = Array.from({ length: TYPED_CHARS }, (_, i) => ({ at: i * KEY_INTERVAL_MS, action: "press", key: letters[i % 26] }));
  const { sends, settled } = await sendKeys(page, steps);
  const drain = await waitForKeys(page, TYPED_CHARS, TYPED_CHARS);
  await settled;
  const keys = await page.evaluate(() => window.__bench.keys);
  return { ...keyLatencies(sends, keys, offset), ...drain };
}

/** One arrow key held: `ARROW_REPEATS` keydowns 20 ms apart, then the keyup. */
async function arrowBurst(page, key, offset) {
  await page.evaluate(() => window.__bench.clearKeys());
  const steps = [...Array.from({ length: ARROW_REPEATS }, (_, i) => ({ at: i * KEY_INTERVAL_MS, action: "down", key })), { at: ARROW_REPEATS * KEY_INTERVAL_MS, action: "up", key }];
  const { sends, settled } = await sendKeys(page, steps);
  const drain = await waitForKeys(page, ARROW_REPEATS, 1);
  await settled;
  const { keys, ups } = await page.evaluate(() => ({ keys: window.__bench.keys, ups: window.__bench.keyups }));
  const keyupSentAt = sends.find((s) => s.action === "up").sentAt;
  // The caret before key i+1 is where key i left it; the last one's is seen at the keyup.
  const after = [...keys.slice(1).map((k) => k.caret), ups[0]?.caret];
  let lastMove = null;
  keys.forEach((k, i) => {
    if (after[i] !== undefined && after[i] !== k.caret) lastMove = k.handledAt;
  });
  const runawayMs = lastMove === null ? null : Math.max(0, lastMove - offset - keyupSentAt);
  return { ...keyLatencies(sends, keys, offset), ...drain, runawayMs: round(runawayMs) };
}

const mergeSpreads = (list) => {
  const spreads = list.filter((s) => s.n > 0);
  return spreads.length === 0 ? { n: 0, p50: null, p95: null, max: null } : { n: spreads.reduce((n, s) => n + s.n, 0), p50: round(percentile(spreads.map((s) => s.p50), 50)), p95: round(Math.max(...spreads.map((s) => s.p95))), max: round(Math.max(...spreads.map((s) => s.max))) };
};

/** Typing, then an arrow held left and one held right, in a field with text enough to move through. */
async function measureInput(page, offset) {
  await page.locator(COMPOSER).fill("");
  await page.locator(COMPOSER).focus();
  const typing = await typeBurst(page, offset);
  await page.locator(COMPOSER).fill("あいうえお ".repeat(30));
  await page.locator(COMPOSER).focus();
  await page.keyboard.press("End");
  const left = await arrowBurst(page, "ArrowLeft", offset);
  const right = await arrowBurst(page, "ArrowRight", offset);
  await page.locator(COMPOSER).fill("");
  return {
    typing: { handler: typing.handler, raf: typing.raf, frame: typing.frame, perKeyMs: typing.perKeyMs, drained: typing.drained },
    arrows: { handler: mergeSpreads([left.handler, right.handler]), raf: mergeSpreads([left.raf, right.raf]), frame: mergeSpreads([left.frame, right.frame]), perKeyMs: round((left.perKeyMs + right.perKeyMs) / 2), drained: left.drained && right.drained },
    runawayMs: round(Math.max(left.runawayMs ?? 0, right.runawayMs ?? 0)),
    runawayLeftRightMs: [left.runawayMs, right.runawayMs],
  };
}

// ---- the measurements --------------------------------------------------------------------------

function frameStats(w, wallMs) {
  const intervals = w.frames.slice(1).map((t, i) => t - w.frames[i]);
  return {
    fps: round(w.frames.length / (wallMs / 1000)),
    interval: spread(intervals),
    framesOver50ms: intervals.filter((x) => x >= 50).length,
    blocksOver50ms: w.blocks.filter((x) => x >= 50).length,
    maxBlockMs: round(w.blocks.length ? Math.max(...w.blocks) : 0),
    animations: w.animations,
  };
}

/** A window of `ms` ms (the page does what it does by itself): frames, blocks and CPU. */
async function measureWindow(page, proc, ms) {
  await page.evaluate(() => window.__bench.begin());
  const a = cpuSample(proc);
  const t0 = performance.now();
  await sleep(ms);
  const b = cpuSample(proc);
  const wall = performance.now() - t0;
  const w = await page.evaluate(() => window.__bench.end());
  return { ...frameStats(w, wall), ...cpuBetween(a, b) };
}

async function measureStream(page, proc, offset, session, o) {
  await page.evaluate(([s, max]) => window.__yhtye.bench.startStream(s, 20, max), [session, o.streamMs + 60_000]);
  await sleep(500);
  await page.evaluate(() => window.__bench.begin());
  const a = cpuSample(proc);
  const t0 = performance.now();
  const input = await measureInput(page, offset);
  const b = cpuSample(proc);
  const wall = performance.now() - t0;
  const w = await page.evaluate(() => window.__bench.end());
  const stream = await page.evaluate(() => window.__yhtye.bench.stopStream());
  await page.evaluate((s) => window.__yhtye.bench.settleStream(s), session);
  await sleep(300);
  return { input, window: { ...frameStats(w, wall), ...cpuBetween(a, b) }, chunks: { emitted: stream.emitted, perSecond: round(stream.emitted / (stream.elapsedMs / 1000)), chars: stream.chars } };
}

async function measureCommits(page, reps) {
  const out = {};
  for (const [name, kind] of [["longAnswer", "long-answer"], ["help", "help"], ["taskResult", "task-result"]]) {
    const runs = [];
    for (let i = 0; i < reps; i++) runs.push(await page.evaluate((k) => window.__bench.measureInject(k), kind));
    out[name] = { maxBlockMs: round(percentile(runs.map((r) => r.maxBlockMs), 50)), worstBlockMs: round(Math.max(...runs.map((r) => r.maxBlockMs))), paintMs: round(percentile(runs.map((r) => r.paintMs), 50)), runs: runs.map((r) => ({ maxBlockMs: round(r.maxBlockMs), paintMs: round(r.paintMs) })) };
  }
  return out;
}

/** All the measurements for one conversation length, with the animations running (`on`) or stopped (`off`). */
async function measureLength(page, proc, offset, o, anim) {
  const result = {};
  const load = () => os.loadavg()[0];
  const style = anim === "off" ? await page.addStyleTag({ content: NO_ANIMATION_CSS }) : null;
  await settleStyle(page);
  if (o.sections.includes("idle")) result.idle = { load1: round(load(), 2), ...(await measureWindow(page, proc, o.idleMs)) };
  if (o.sections.includes("input")) result.input = { load1: round(load(), 2), ...(await measureInput(page, offset)) };
  const want = (name) => o.sections.includes("stream") || o.sections.includes(name);
  if (want("stream-shown")) {
    result.streamVisible = { load1: round(load(), 2), ...(await measureStream(page, proc, offset, await page.evaluate(() => window.__yhtye.bench.orchestrator), o)) };
  }
  if (want("stream-other")) {
    result.streamOther = { load1: round(load(), 2), ...(await measureStream(page, proc, offset, await page.evaluate(() => window.__yhtye.bench.otherSession), o)) };
  }
  if (o.sections.includes("commit")) result.commit = { load1: round(load(), 2), ...(await measureCommits(page, o.reps)) };
  if (style) await style.evaluate((el) => el.remove());
  return result;
}

/** The same key driving into a bare textarea: the floor of what this setup can measure. */
async function measureControl(browser, o) {
  const page = await browser.newPage({ viewport: VIEWPORT });
  await page.setContent(`<textarea aria-label="オーケストレータへのメッセージ" rows="3"></textarea>`);
  await page.evaluate(installBench);
  await page.evaluate((sel) => window.__bench.watch(sel), COMPOSER);
  const offset = await clockOffset(page);
  const input = await measureInput(page, offset);
  await page.close();
  return { load1: round(os.loadavg()[0], 2), ...input };
}

async function measureBrowser(kind, browser, baseUrl, o, before) {
  const rows = [];
  if (o.sections.includes("input")) rows.push({ browser: kind, n: 0, shape: { items: 0, domNodes: 0 }, results: { control: { input: await measureControl(browser, o) } } });
  for (const n of o.n) {
    const page = await browser.newPage({ viewport: VIEWPORT });
    page.on("pageerror", (e) => console.error(`  [${kind} n=${n}] page error: ${e.message}`));
    const loadStart = performance.now();
    await page.goto(`${baseUrl}/layout-harness.html?scenario=long&exchanges=${n}&mix=real`);
    await page.waitForFunction(() => document.documentElement.dataset.ready === "long", null, { timeout: READY_TIMEOUT_MS });
    const loadedMs = performance.now() - loadStart;
    await page.evaluate(installBench);
    // The harness' snapshot starts at seq 0, so the whole log is folded already; this is for a core with a real snapshot.
    const historyPages = await page.evaluate(() => window.__yhtye.bench.loadAllHistory());
    await page.evaluate((sel) => window.__bench.watch(sel), COMPOSER);
    await sleep(1500);
    const proc = pageProcess(kind, before);
    const offset = await clockOffset(page);
    const shape = await page.evaluate(() => ({ items: window.__yhtye.bench.items(), historyStart: window.__yhtye.bench.historyStart(), domNodes: document.querySelectorAll("*").length }));
    console.log(`  ${kind} n=${n}: ${shape.items} items, ${shape.domNodes} DOM nodes, loaded in ${round(loadedMs / 1000)}s (history pages: ${historyPages}), web process pid ${proc ?? "?"}`);
    const states = o.anim === "both" ? ["on", "off"] : [o.anim];
    const results = {};
    for (const anim of states) {
      console.log(`    animations ${anim} ...`);
      results[anim] = await measureLength(page, proc, offset, o, anim);
    }
    // The idle stretch is always measured both ways.
    if (o.anim !== "both" && o.sections.includes("idle")) {
      const other = o.anim === "on" ? "off" : "on";
      const style = other === "off" ? await page.addStyleTag({ content: NO_ANIMATION_CSS }) : null;
      await settleStyle(page);
      results[other] = { idle: { load1: round(os.loadavg()[0], 2), ...(await measureWindow(page, proc, o.idleMs)) } };
      if (style) await style.evaluate((el) => el.remove());
    }
    rows.push({ browser: kind, n, shape, loadedMs: round(loadedMs, 0), clockOffsetMs: round(offset), results });
    await page.close();
  }
  return rows;
}

// ---- output -------------------------------------------------------------------------------------

function table(title, header, rows) {
  if (rows.length === 0) return;
  const cells = [header, ...rows].map((r) => r.map(fmt));
  const widths = header.map((_, c) => Math.max(...cells.map((r) => r[c].length)));
  const line = (r) => r.map((cell, c) => cell.padEnd(widths[c])).join("  ").trimEnd();
  console.log(`\n${title}`);
  console.log(line(cells[0]));
  console.log(widths.map((w) => "-".repeat(w)).join("  "));
  cells.slice(1).forEach((r) => console.log(line(r)));
}

function printTables(all) {
  const each = (pick) => all.flatMap((r) => Object.entries(r.results).flatMap(([anim, res]) => pick(r, anim, res)));
  table(
    "IDLE (5 s of nothing happening)  rAF = ms between frames p50/p95/max; blocks = main-thread blocks of 50 ms or more; cpu = web process main thread / whole process",
    ["browser", "n", "items", "DOM", "anim", "running", "fps", "rAF p50/p95/max", "frames>50", "blocks>50", "max block", "cpu main/all %", "load"],
    each((r, anim, res) => (res.idle ? [[r.browser, r.n, r.shape.items, r.shape.domNodes, anim, res.idle.animations, res.idle.fps, triple(res.idle.interval), res.idle.framesOver50ms, res.idle.blocksOver50ms, res.idle.maxBlockMs, `${fmt(res.idle.mainPct)}/${fmt(res.idle.processPct)}`, res.idle.load1]] : [])),
  );
  const inputRow = (r, anim, label, x, extra = []) => [r.browser, r.n, anim, label, triple(x.typing.raf), x.typing.perKeyMs, triple(x.arrows.handler), triple(x.arrows.raf), x.arrows.perKeyMs, x.runawayMs, ...extra];
  table(
    "INPUT  ms from the key being sent to the next frame (rAF) p50/p95/max; ms/char, ms/arrow = main-thread time one key costs; runaway = ms the caret kept moving after the key was let go",
    ["browser", "n", "anim", "state", "typing->rAF", "ms/char", "arrows->page", "arrows->rAF", "ms/arrow", "runaway", "chunks/s", "stream cpu main %", "load"],
    each((r, anim, res) => [
      ...(anim === "control" ? [inputRow(r, "-", "control: bare textarea", res.input, ["-", "-", res.input.load1])] : []),
      ...(res.input && anim !== "control" ? [inputRow(r, anim, "quiet", res.input, ["-", "-", res.input.load1])] : []),
      ...(res.streamVisible ? [inputRow(r, anim, "stream: shown chat", res.streamVisible.input, [res.streamVisible.chunks.perSecond, res.streamVisible.window.mainPct, res.streamVisible.load1])] : []),
      ...(res.streamOther ? [inputRow(r, anim, "stream: other task", res.streamOther.input, [res.streamOther.chunks.perSecond, res.streamOther.window.mainPct, res.streamOther.load1])] : []),
    ]),
  );
  table(
    "COMMIT  longest block of the main thread (ms, median of the repetitions) / ms until the frame after, for one durable event",
    ["browser", "n", "anim", "long answer", "help", "task result", "worst block", "load"],
    each((r, anim, res) => (res.commit ? [[r.browser, r.n, anim, `${fmt(res.commit.longAnswer.maxBlockMs)} / ${fmt(res.commit.longAnswer.paintMs)}`, `${fmt(res.commit.help.maxBlockMs)} / ${fmt(res.commit.help.paintMs)}`, `${fmt(res.commit.taskResult.maxBlockMs)} / ${fmt(res.commit.taskResult.paintMs)}`, Math.max(res.commit.longAnswer.worstBlockMs, res.commit.help.worstBlockMs, res.commit.taskResult.worstBlockMs), res.commit.load1]] : [])),
  );
}

// ---- the page's server and the browsers ---------------------------------------------------------

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

async function startServer(o) {
  if (o.url) return { url: o.url, close: async () => undefined };
  const vite = await import("vite");
  const react = (await import("@vitejs/plugin-react")).default;
  const port = await freePort();
  const common = { root, configFile: false, plugins: [react()], logLevel: "warn", clearScreen: false };
  if (o.mode === "dev") {
    const server = await vite.createServer({ ...common, server: { host: "127.0.0.1", port, strictPort: true, hmr: false, watch: null } });
    await server.listen();
    return { url: `http://127.0.0.1:${port}`, close: () => server.close() };
  }
  const outDir = path.join(o.out, ".harness-dist");
  console.log("building the harness (production) ...");
  await vite.build({ ...common, build: { outDir, emptyOutDir: true, rollupOptions: { input: path.join(root, "layout-harness.html") } } });
  const server = await vite.preview({ ...common, build: { outDir }, preview: { host: "127.0.0.1", port, strictPort: true } });
  return { url: `http://127.0.0.1:${port}`, close: () => new Promise((resolve) => server.httpServer.close(() => resolve())) };
}

function launchOptions(kind) {
  if (kind === "webkit") return { launcher: webkit, options: { headless: true } };
  const executablePath = process.env.YHTYE_CHROMIUM ?? CHROMIUM_CANDIDATES.find((p) => existsSync(p));
  return { launcher: chromium, options: { headless: true, executablePath, args: ["--no-sandbox"] } };
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  mkdirSync(o.out, { recursive: true });
  const startedAt = new Date();
  const loadStart = os.loadavg();
  const server = await startServer(o);
  const all = [];
  const versions = {};
  try {
    for (const kind of o.browsers) {
      const before = new Set(procPids());
      const { launcher, options } = launchOptions(kind);
      let browser;
      try {
        browser = await launcher.launch(options);
      } catch (e) {
        throw new Error(`could not start ${kind}: ${e.message}\nWebKit: pnpm exec playwright-core install webkit (on a distribution that is not Ubuntu see docs/DEVELOPMENT.md, "Benchmark of a long chat")`);
      }
      versions[kind] = browser.version();
      console.log(`\n${kind} ${versions[kind]}  (${o.mode} build, loadavg ${loadStart.map((x) => x.toFixed(1)).join(" ")})`);
      try {
        all.push(...(await measureBrowser(kind, browser, server.url, o, before)));
      } finally {
        await browser.close();
      }
    }
  } finally {
    await server.close();
  }
  printTables(all);
  const loadEnd = os.loadavg();
  const file = path.join(o.out, `long-chat-${startedAt.toISOString().replace(/[:.]/g, "-")}.json`);
  const meta = { startedAt: startedAt.toISOString(), mode: o.mode, versions, cpus: os.cpus().length, loadavgStart: loadStart.map((x) => round(x, 2)), loadavgEnd: loadEnd.map((x) => round(x, 2)), node: process.version, platform: `${process.platform} ${os.release()}`, args: o };
  writeFileSync(file, JSON.stringify({ meta, rows: all }, null, 2));
  console.log(`\nloadavg (1 / 5 / 15 min) at start ${meta.loadavgStart.join(" ")}, at end ${meta.loadavgEnd.join(" ")} on ${meta.cpus} CPUs; a loaded machine inflates every number`);
  console.log(`saved ${path.relative(root, file)}`);
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e.stack ?? e);
    process.exit(1);
  },
);
