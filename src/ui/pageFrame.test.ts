// The page is one fixed frame: it never scrolls, only the panels inside it do. jsdom computes no
// layout, so this reads the rules themselves; `pnpm check:layout` (scripts/check-layout.mjs)
// looks at the real thing in a browser.

import { describe, expect, it } from "vitest";

import appCss from "../App.css?raw";
import tokensCss from "../styles/tokens.css?raw";
import iconCss from "./icon.css?raw";
import sidebarCss from "./sidebar.css?raw";

interface Rule {
  selector: string;
  declarations: Map<string, string>;
}

/** The rules of a stylesheet, at-rules flattened, one per selector of a list. */
function parseRules(css: string): Rule[] {
  const rules: Rule[] = [];
  const walk = (text: string) => {
    let rest = text;
    for (;;) {
      const open = rest.indexOf("{");
      if (open < 0) return;
      const head = rest.slice(0, open).trim();
      let depth = 1;
      let close = open + 1;
      while (depth > 0 && close < rest.length) {
        if (rest[close] === "{") depth++;
        if (rest[close] === "}") depth--;
        close++;
      }
      const body = rest.slice(open + 1, close - 1);
      if (head.startsWith("@")) {
        walk(body);
      } else {
        const declarations = new Map<string, string>();
        for (const part of body.split(";")) {
          const colon = part.indexOf(":");
          if (colon > 0) declarations.set(part.slice(0, colon).trim(), part.slice(colon + 1).trim());
        }
        for (const selector of head.split(",")) rules.push({ selector: selector.trim().replace(/\s+/g, " "), declarations });
      }
      rest = rest.slice(close);
    }
  };
  walk(css.replace(/\/\*[\s\S]*?\*\//g, "").replace(/@import[^;]*;/g, ""));
  return rules;
}

const RULES = [tokensCss, appCss, sidebarCss, iconCss].flatMap(parseRules);

/** The value `selector` gets for `property`: the last rule that sets it wins. */
function valueOf(selector: string, property: string): string | undefined {
  return RULES.filter((r) => r.selector === selector && r.declarations.has(property)).at(-1)?.declarations.get(property);
}

describe("the page frame", () => {
  it("fills the window and does not scroll: html, body and #root are 100% high with overflow hidden", () => {
    for (const selector of ["html", "body", "#root"]) {
      expect(valueOf(selector, "height"), `${selector} height`).toBe("100%");
      expect(valueOf(selector, "overflow"), `${selector} overflow`).toBe("hidden");
      expect(valueOf(selector, "margin"), `${selector} margin`).toBe("0");
    }
  });

  it("makes .app exactly the window, positioned, and no scroll container where `overflow: clip` is known", () => {
    expect(valueOf(".app", "height")).toBe("100%");
    expect(valueOf(".app", "position")).toBe("relative");
    expect(valueOf(".app", "overflow")).toBe("clip");
    expect(appCss).toMatch(/overflow:\s*hidden;\s*overflow:\s*clip/); // `hidden` stays for a browser that does not know `clip`
    // A min-width makes the page scroll sideways when the window is zoomed to fewer CSS pixels.
    expect(valueOf(".app", "min-width")).toBeUndefined();
    expect(valueOf(".app", "min-height")).toBeUndefined();
  });

  it("has no panel as big as the viewport units (100vh / 100vw): the frame is 100% of #root", () => {
    const sized = RULES.filter((r) => /\b(100vh|100vw)\b/.test(r.declarations.get("height") ?? "") || /\b100vw\b/.test(r.declarations.get("width") ?? ""));
    expect(sized.map((r) => r.selector)).toEqual([]);
  });

  it("lets the flex children of the frame shrink: min-height / min-width 0", () => {
    for (const selector of [".body", ".tasks", ".branches", ".scroll", ".settings-body"]) {
      expect(valueOf(selector, "min-height"), `${selector} min-height`).toBe("0");
    }
    for (const selector of [".body", ".workspace", ".scroll", ".settings-main"]) {
      expect(valueOf(selector, "min-width"), `${selector} min-width`).toBe("0");
    }
  });

  it("positions every scroll area, so that an .sr-only label inside is clipped and scrolled with it", () => {
    expect(valueOf(".sr-only", "position")).toBe("absolute");
    const scrollAreas = RULES.filter((r) => ["auto", "scroll"].includes(valueOf(r.selector, "overflow") ?? valueOf(r.selector, "overflow-y") ?? valueOf(r.selector, "overflow-x") ?? ""));
    expect(scrollAreas.length).toBeGreaterThan(0);
    const unpositioned = scrollAreas.map((r) => r.selector).filter((selector) => !["relative", "absolute", "fixed"].includes(valueOf(selector, "position") ?? "static"));
    expect(unpositioned).toEqual([]);
  });
});
