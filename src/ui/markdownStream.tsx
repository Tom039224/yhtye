import { Fragment, type ReactElement, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";

import { BASE_PLUGINS, MARKDOWN_OPTIONS, type RemarkPlugins } from "./markdownOptions";

/**
 * Markdown of a text that grows (an agent's reply while it is streamed).
 *
 * Parsing the whole text for every chunk costs more the longer the reply is
 * (a quadratic total). Blocks that cannot change any more are rendered once and
 * kept; only the end of the text, which can still change, is parsed again.
 *
 * Which blocks cannot change comes from the parser itself, not from a second
 * implementation of Markdown's block rules: the top-level blocks of the end of
 * the text are read from the same parse that renders it (a remark plugin sees
 * the tree). Appending text can change a block that was already complete in
 * two ways, both handled here:
 * - the last line was unfinished: `1. a`, a blank line and `2` is a paragraph,
 *   and `2.` joins it to the list above. So the last block and the one before
 *   it stay open (`LOOKAHEAD`); no other line start changes the block before it.
 * - a definition (`[x]: url`, `[^1]: note`) is used by links in blocks anywhere
 *   in the text. Once one exists the text is rendered whole, as it was before.
 *
 * The parser also reads some blocks in the context of what came before (after
 * an indented code block, `3. a` is a paragraph, not a list). A cut is made only
 * if the pieces, parsed apart, have the blocks they had in the whole (the same
 * types in the same places); otherwise the end stays open a little longer.
 */

/** Blocks at the end that are never closed: the last one is still growing, the one before may join it. */
const LOOKAHEAD = 2;
/** More commits than this in one step would mean the parser disagrees with itself; the rest waits. */
const MAX_COMMITS_PER_STEP = 8;

interface MdNode {
  type: string;
  children?: MdNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
}

/** What the parser says about the top-level blocks of a text. */
export interface Outline {
  /** Where each block starts and ends (`null`: the parser gave no positions). */
  blocks: { type: string; start: number; end: number }[] | null;
  /** The text has a link or footnote definition. */
  definition: boolean;
}

const NO_OUTLINE: Outline = { blocks: null, definition: false };

function isDefinition(node: MdNode): boolean {
  if (node.type === "definition" || node.type === "footnoteDefinition") return true;
  return node.children?.some(isDefinition) ?? false;
}

function outlineOf(root: MdNode): Outline {
  const blocks: { type: string; start: number; end: number }[] = [];
  for (const child of root.children ?? []) {
    const start = child.position?.start.offset;
    const end = child.position?.end.offset;
    if (start === undefined || end === undefined) return { blocks: null, definition: isDefinition(root) };
    blocks.push({ type: child.type, start, end });
  }
  return { blocks, definition: isDefinition(root) };
}

/** The elements of `text` and, from the same parse, its outline. */
export function parseWithOutline(text: string): { element: ReactElement; outline: Outline } {
  let outline = NO_OUTLINE;
  const readOutline = () => (tree: MdNode) => {
    outline = outlineOf(tree);
  };
  const plugins: RemarkPlugins = [...BASE_PLUGINS, readOutline as unknown as RemarkPlugins[number]];
  // `Markdown` of react-markdown is a plain function (no hooks): calling it
  // gives the elements now, which are kept and reused when nothing changed.
  const element = ReactMarkdown({ ...MARKDOWN_OPTIONS, remarkPlugins: plugins, children: text });
  return { element, outline };
}

/** The elements of `text`. */
export function renderMarkdown(text: string): ReactElement {
  return ReactMarkdown({ ...MARKDOWN_OPTIONS, remarkPlugins: BASE_PLUGINS, children: text });
}

function lineStart(text: string, offset: number): number {
  let i = offset;
  while (i > 0 && text[i - 1] !== "\n" && text[i - 1] !== "\r") i--;
  return i;
}

/**
 * Where the closed blocks at the start of `text` end (the offset of the line
 * where the first open block starts), 0 when no block is closed.
 */
export function closedLength(text: string, outline: Outline): number {
  const blocks = outline.blocks;
  if (!blocks || blocks.length <= LOOKAHEAD) return 0;
  const open = blocks.length - LOOKAHEAD;
  const cut = lineStart(text, blocks[open].start);
  // Blocks never share a line; if the parser says otherwise, wait.
  return cut > 0 && cut >= blocks[open - 1].end ? cut : 0;
}

type Parsed = ReturnType<typeof parseWithOutline>;

/** Do `part`'s blocks match `whole`'s from `from` on, `shift` characters earlier? */
function sameBlocks(whole: NonNullable<Outline["blocks"]>, from: number, count: number, shift: number, part: Outline["blocks"]): boolean {
  if (!part || part.length !== count) return false;
  return part.every((b, i) => {
    const w = whole[from + i];
    return b.type === w.type && b.start === w.start - shift && b.end === w.end - shift;
  });
}

/**
 * The closed start of `text` and the rest, each parsed apart, if cutting there
 * changes no block; `null` when there is nothing to close or cutting is not safe.
 */
function closeStart(text: string, outline: Outline): { cut: number; head: Parsed; rest: Parsed } | null {
  const cut = closedLength(text, outline);
  const blocks = outline.blocks;
  if (cut === 0 || !blocks) return null;
  const open = blocks.length - LOOKAHEAD;
  const head = parseWithOutline(text.slice(0, cut));
  const rest = parseWithOutline(text.slice(cut));
  const agree = !head.outline.definition && !rest.outline.definition && sameBlocks(blocks, 0, open, 0, head.outline.blocks) && sameBlocks(blocks, open, LOOKAHEAD, cut, rest.outline.blocks);
  return agree ? { cut, head, rest } : null;
}

export interface StreamState {
  /** The text this state was made for. */
  readonly text: string;
  /** Source of each closed block, in order; `text` starts with their concatenation. */
  readonly closed: readonly string[];
  /** The closed blocks as elements; they are the same objects from one step to the next. */
  readonly closedElements: readonly ReactElement[];
  /** Rendered as one piece: a definition needs the whole text. */
  readonly whole: boolean;
  /** Everything, ready to be put in the page. */
  readonly output: ReactElement;
}

/** Top-level blocks of the whole text are separated by a newline text node; so are the pieces. */
function closedBlock(index: number, element: ReactElement): ReactElement {
  return (
    <Fragment key={index}>
      {index > 0 ? "\n" : null}
      {element}
    </Fragment>
  );
}

function compose(closedElements: readonly ReactElement[], tail: ReactNode): ReactElement {
  return (
    <>
      {closedElements}
      <Fragment key="tail">
        {closedElements.length > 0 ? "\n" : null}
        {tail}
      </Fragment>
    </>
  );
}

export const EMPTY_STREAM: StreamState = { text: "", closed: [], closedElements: [], whole: false, output: <></> };

function wholeState(text: string, element: ReactElement = renderMarkdown(text)): StreamState {
  return { text, closed: [], closedElements: [], whole: true, output: element };
}

/**
 * The state for `text`, given the one for an earlier text. If `text` goes on
 * from the earlier one, only its open end is parsed again; if not, everything is.
 * Pure: the result depends on `text` only (`prev` is a cache), so a state may
 * be dropped at any time.
 */
export function advance(prev: StreamState, text: string): StreamState {
  if (text === prev.text) return prev;
  const base = text.startsWith(prev.text) ? prev : EMPTY_STREAM;
  if (base.whole) return wholeState(text);

  let closed = base.closed;
  let closedElements = base.closedElements;
  let offset = closed.reduce((n, s) => n + s.length, 0);
  let parsed = parseWithOutline(text.slice(offset));
  for (let commits = 0; ; commits++) {
    const { element, outline } = parsed;
    if (outline.definition) return wholeState(text, offset === 0 ? element : undefined);
    const tail = text.slice(offset);
    const close = commits < MAX_COMMITS_PER_STEP ? closeStart(tail, outline) : null;
    if (!close) return { text, closed, closedElements, whole: false, output: compose(closedElements, element) };
    closed = [...closed, tail.slice(0, close.cut)];
    closedElements = [...closedElements, closedBlock(closedElements.length, close.head.element)];
    offset += close.cut;
    parsed = close.rest;
  }
}

/**
 * The output of the latest steps, by the text it is for. When the text ends and
 * is stored, the same text is shown by a new component; it takes the elements
 * that were made while it was streamed instead of parsing it all again.
 */
const RECENT_OUTPUTS = 6;
const recent = new Map<string, ReactElement>();

/** Remembers `output` for `text`, forgetting what the same stream remembered for its `previous` text. */
export function rememberOutput(previous: string | null, text: string, output: ReactElement): void {
  if (previous !== null) recent.delete(previous);
  recent.delete(text);
  recent.set(text, output);
  while (recent.size > RECENT_OUTPUTS) recent.delete(recent.keys().next().value as string);
}

/** The output remembered for `text`, if any. */
export function recallOutput(text: string): ReactElement | undefined {
  return recent.size === 0 ? undefined : recent.get(text);
}
