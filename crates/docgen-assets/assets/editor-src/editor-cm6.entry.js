// docgen-rs in-browser editor — CodeMirror 6 ESM entry.
//
// This is a faithful vanilla port of the original Svelte docgen editor
// (DocEditorView / DocSourceEditor / DocEditorPreview + the editor-* helpers).
// It is bundled (esbuild IIFE) into a vendored artifact by the caller; this
// file is the ESM source only — do not run a build here.
//
// Mount contract:
//   <div id="docgen-editor-app"
//        data-doc-path="guide/intro.md"
//        data-view-path="/guide/intro"
//        data-title="Introduction"
//        data-base=""></div>
//
// Endpoints (relative to data-base):
//   GET  ${base}/__docgen/source?path=<docPath>
//          -> { path, source, disk_hash, head_source }
//   POST ${base}/__docgen/preview { path, source } -> { html }
//   PUT  ${base}/__docgen/source  { path, source, disk_hash } -> { path, disk_hash }
//          (409 if file changed on disk)
//   GET  ${base}/search-index.json -> [{ slug, title, text }, ...]

import { EditorView, basicSetup } from "codemirror";
import {
  keymap,
  Decoration,
  ViewPlugin,
} from "@codemirror/view";
import { Compartment, Prec, RangeSetBuilder } from "@codemirror/state";
import { markdown } from "@codemirror/lang-markdown";
import { unifiedMergeView } from "@codemirror/merge";
import {
  autocompletion,
  acceptCompletion,
  completionStatus,
} from "@codemirror/autocomplete";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

// ---------------------------------------------------------------------------
// Theme — port of the var-driven "default" theme from editor-themes.ts.
// ---------------------------------------------------------------------------

function buildVarTheme() {
  const view = EditorView.theme({
    "&": {
      height: "100%",
      color: "var(--text)",
      backgroundColor: "var(--surface)",
    },
    ".cm-scroller": {
      fontFamily: "var(--font-mono)",
      fontSize: "13px",
      lineHeight: "1.55",
    },
    ".cm-content": { caretColor: "var(--accent)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      backgroundColor: "var(--accent-soft) !important",
    },
    ".cm-activeLine": { backgroundColor: "var(--bg-soft)" },
    ".cm-activeLineGutter": {
      backgroundColor: "transparent",
      color: "var(--text)",
    },
    ".cm-gutters": {
      backgroundColor: "var(--bg-elev)",
      color: "var(--text-mute)",
      borderRight: "1px solid var(--border)",
    },
    ".cm-focused": { outline: "none" },
    ".cm-deletedChunk, .cm-changedLine, .cm-insertedLine": {
      backgroundColor: "var(--warn-soft)",
    },
    ".cm-wikilink-bracket, .cm-wikilink-bracket > span": {
      color: "var(--text-mute) !important",
    },
    ".cm-wikilink-target, .cm-wikilink-target > span": {
      color: "var(--info) !important",
    },
    ".cm-wikilink-label, .cm-wikilink-label > span": {
      color: "var(--text) !important",
      fontWeight: "600",
    },
  });

  const highlight = HighlightStyle.define([
    { tag: t.heading1, color: "var(--accent)", fontWeight: "700" },
    { tag: t.heading2, color: "var(--accent)", fontWeight: "700" },
    { tag: t.heading3, color: "var(--accent)", fontWeight: "600" },
    {
      tag: [t.heading4, t.heading5, t.heading6],
      color: "var(--accent)",
      fontWeight: "600",
    },
    { tag: t.strong, color: "var(--text)", fontWeight: "700" },
    { tag: t.emphasis, color: "var(--text)", fontStyle: "italic" },
    {
      tag: t.strikethrough,
      color: "var(--text-mute)",
      textDecoration: "line-through",
    },
    { tag: t.link, color: "var(--info)" },
    { tag: t.url, color: "var(--info)", textDecoration: "underline" },
    { tag: t.monospace, color: "var(--warn)" },
    { tag: t.quote, color: "var(--talk)", fontStyle: "italic" },
    { tag: t.processingInstruction, color: "var(--accent)" },
    { tag: t.meta, color: "var(--text-mute)" },
    { tag: t.comment, color: "var(--text-mute)", fontStyle: "italic" },
    { tag: t.keyword, color: "var(--talk)" },
    { tag: [t.atom, t.bool, t.special(t.variableName)], color: "var(--info)" },
    { tag: [t.number, t.literal], color: "var(--warn)" },
    { tag: [t.string, t.special(t.string)], color: "var(--info)" },
    { tag: [t.regexp, t.escape], color: "var(--warn)" },
    { tag: t.contentSeparator, color: "var(--text-mute)" },
    { tag: t.invalid, color: "var(--text-error)" },
  ]);

  return [view, syntaxHighlighting(highlight)];
}

// A named-palette theme (faithful port of editor-themes.ts `buildTheme`). Each
// of the 8 selectable editor themes is one of these over a fixed palette.
function buildNamedTheme(p, dark) {
  const view = EditorView.theme(
    {
      "&": { height: "100%", color: p.fg, backgroundColor: p.bg },
      ".cm-scroller": {
        fontFamily: "var(--font-mono)",
        fontSize: "13px",
        lineHeight: "1.55",
      },
      ".cm-content": { caretColor: p.cursor },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: p.cursor },
      "&.cm-focused .cm-selectionBackground, ::selection": {
        backgroundColor: `${p.selection} !important`,
      },
      ".cm-selectionBackground": {
        backgroundColor: `${p.selection} !important`,
      },
      ".cm-activeLine": { backgroundColor: p.activeLine },
      ".cm-activeLineGutter": { backgroundColor: "transparent", color: p.fg },
      ".cm-gutters": {
        backgroundColor: p.gutterBg,
        color: p.gutterFg,
        borderRight: `1px solid ${p.border}`,
      },
      ".cm-focused": { outline: "none" },
      ".cm-deletedChunk, .cm-changedLine, .cm-insertedLine": {
        backgroundColor: dark ? "rgba(255,200,80,0.10)" : "rgba(180,120,20,0.10)",
      },
      ".cm-wikilink-bracket, .cm-wikilink-bracket > span": {
        color: `${p.meta} !important`,
      },
      ".cm-wikilink-target, .cm-wikilink-target > span": {
        color: `${p.link} !important`,
      },
      ".cm-wikilink-label, .cm-wikilink-label > span": {
        color: `${p.strong} !important`,
        fontWeight: "600",
      },
    },
    { dark },
  );

  const highlight = HighlightStyle.define([
    { tag: t.heading1, color: p.heading, fontWeight: "700" },
    { tag: t.heading2, color: p.heading, fontWeight: "700" },
    { tag: t.heading3, color: p.heading, fontWeight: "600" },
    { tag: [t.heading4, t.heading5, t.heading6], color: p.heading, fontWeight: "600" },
    { tag: t.strong, color: p.strong, fontWeight: "700" },
    { tag: t.emphasis, color: p.emphasis, fontStyle: "italic" },
    { tag: t.strikethrough, color: p.meta, textDecoration: "line-through" },
    { tag: t.link, color: p.link },
    { tag: t.url, color: p.url, textDecoration: "underline" },
    { tag: t.monospace, color: p.code },
    { tag: t.quote, color: p.quote, fontStyle: "italic" },
    { tag: t.processingInstruction, color: p.list },
    { tag: t.meta, color: p.meta },
    { tag: t.comment, color: p.comment, fontStyle: "italic" },
    { tag: t.keyword, color: p.keyword },
    { tag: [t.atom, t.bool, t.special(t.variableName)], color: p.atom },
    { tag: [t.number, t.literal], color: p.number },
    { tag: [t.string, t.special(t.string)], color: p.string },
    { tag: [t.regexp, t.escape], color: p.regexp },
    { tag: t.contentSeparator, color: p.meta },
    { tag: t.invalid, color: p.error },
  ]);

  return [view, syntaxHighlighting(highlight)];
}

// --- Palettes (verbatim from editor-themes.ts) ---------------------------
const PALETTES = {
  "github-light": { bg: "#ffffff", fg: "#1f2328", gutterBg: "#f6f8fa", gutterFg: "#8c959f", activeLine: "#f6f8fa", activeLineGutter: "#eaeef2", selection: "rgba(84,174,255,0.30)", cursor: "#0969da", border: "#d0d7de", heading: "#0550ae", strong: "#1f2328", emphasis: "#1f2328", link: "#0969da", url: "#0969da", code: "#953800", quote: "#6f42c1", list: "#0550ae", meta: "#6e7781", comment: "#6e7781", keyword: "#cf222e", atom: "#0550ae", string: "#0a3069", number: "#0550ae", regexp: "#116329", error: "#cf222e" },
  "github-dark": { bg: "#0d1117", fg: "#e6edf3", gutterBg: "#0d1117", gutterFg: "#6e7681", activeLine: "#161b22", activeLineGutter: "#161b22", selection: "rgba(56,139,253,0.30)", cursor: "#58a6ff", border: "#30363d", heading: "#79c0ff", strong: "#e6edf3", emphasis: "#e6edf3", link: "#58a6ff", url: "#58a6ff", code: "#ffa657", quote: "#d2a8ff", list: "#79c0ff", meta: "#8b949e", comment: "#8b949e", keyword: "#ff7b72", atom: "#79c0ff", string: "#a5d6ff", number: "#79c0ff", regexp: "#7ee787", error: "#ff7b72" },
  monokai: { bg: "#272822", fg: "#f8f8f2", gutterBg: "#272822", gutterFg: "#75715e", activeLine: "#3e3d32", activeLineGutter: "#3e3d32", selection: "rgba(73,72,62,0.99)", cursor: "#f8f8f0", border: "#3e3d32", heading: "#a6e22e", strong: "#f8f8f2", emphasis: "#f8f8f2", link: "#66d9ef", url: "#66d9ef", code: "#e6db74", quote: "#ae81ff", list: "#fd971f", meta: "#75715e", comment: "#75715e", keyword: "#f92672", atom: "#ae81ff", string: "#e6db74", number: "#ae81ff", regexp: "#fd971f", error: "#f92672" },
  "solarized-light": { bg: "#fdf6e3", fg: "#586e75", gutterBg: "#eee8d5", gutterFg: "#93a1a1", activeLine: "#eee8d5", activeLineGutter: "#e3dbb6", selection: "rgba(7,54,66,0.15)", cursor: "#586e75", border: "#e3dbb6", heading: "#268bd2", strong: "#073642", emphasis: "#586e75", link: "#268bd2", url: "#268bd2", code: "#d33682", quote: "#6c71c4", list: "#cb4b16", meta: "#93a1a1", comment: "#93a1a1", keyword: "#859900", atom: "#d33682", string: "#2aa198", number: "#d33682", regexp: "#dc322f", error: "#dc322f" },
  "solarized-dark": { bg: "#002b36", fg: "#93a1a1", gutterBg: "#073642", gutterFg: "#586e75", activeLine: "#073642", activeLineGutter: "#0a3a47", selection: "rgba(238,232,213,0.15)", cursor: "#93a1a1", border: "#0a3a47", heading: "#268bd2", strong: "#fdf6e3", emphasis: "#93a1a1", link: "#268bd2", url: "#268bd2", code: "#d33682", quote: "#6c71c4", list: "#cb4b16", meta: "#586e75", comment: "#586e75", keyword: "#859900", atom: "#d33682", string: "#2aa198", number: "#d33682", regexp: "#dc322f", error: "#dc322f" },
  dracula: { bg: "#282a36", fg: "#f8f8f2", gutterBg: "#282a36", gutterFg: "#6272a4", activeLine: "#343746", activeLineGutter: "#343746", selection: "rgba(68,71,90,0.99)", cursor: "#f8f8f0", border: "#44475a", heading: "#bd93f9", strong: "#f8f8f2", emphasis: "#f8f8f2", link: "#8be9fd", url: "#8be9fd", code: "#f1fa8c", quote: "#ff79c6", list: "#ffb86c", meta: "#6272a4", comment: "#6272a4", keyword: "#ff79c6", atom: "#bd93f9", string: "#f1fa8c", number: "#bd93f9", regexp: "#ffb86c", error: "#ff5555" },
  "one-dark": { bg: "#282c34", fg: "#abb2bf", gutterBg: "#282c34", gutterFg: "#495162", activeLine: "#2c313a", activeLineGutter: "#2c313a", selection: "rgba(58,103,178,0.55)", cursor: "#528bff", border: "#3e4451", heading: "#e06c75", strong: "#abb2bf", emphasis: "#abb2bf", link: "#61afef", url: "#61afef", code: "#98c379", quote: "#56b6c2", list: "#d19a66", meta: "#5c6370", comment: "#5c6370", keyword: "#c678dd", atom: "#d19a66", string: "#98c379", number: "#d19a66", regexp: "#56b6c2", error: "#e06c75" },
};

// Selectable editor themes, in menu order (verbatim labels/swatches from the
// original registry). "default" tracks the site theme via CSS vars; the rest are
// fixed named palettes independent of the page's light/dark setting.
const EDITOR_THEMES = [
  { id: "default", label: "docgen (matches theme)", swatch: "var(--accent)" },
  { id: "github-light", label: "GitHub Light", swatch: "#0969da" },
  { id: "github-dark", label: "GitHub Dark", swatch: "#58a6ff" },
  { id: "monokai", label: "Monokai", swatch: "#a6e22e" },
  { id: "solarized-light", label: "Solarized Light", swatch: "#268bd2" },
  { id: "solarized-dark", label: "Solarized Dark", swatch: "#2aa198" },
  { id: "dracula", label: "Dracula", swatch: "#bd93f9" },
  { id: "one-dark", label: "One Dark", swatch: "#e06c75" },
];
const DARK_THEME_IDS = new Set([
  "github-dark", "monokai", "solarized-dark", "dracula", "one-dark",
]);

function buildEditorTheme(id) {
  if (id === "default" || !PALETTES[id]) return buildVarTheme();
  return buildNamedTheme(PALETTES[id], DARK_THEME_IDS.has(id));
}

// ---------------------------------------------------------------------------
// Wikilink highlighter — port of editor-wikilinks.ts.
// ---------------------------------------------------------------------------

// [[target]] or [[target|label]]
const WIKILINK_RE = /\[\[([^\]\n|]+?)(?:\|([^\]\n]+?))?\]\]/g;

const bracketMark = Decoration.mark({ class: "cm-wikilink-bracket" });
const targetMark = Decoration.mark({ class: "cm-wikilink-target" });
const labelMark = Decoration.mark({ class: "cm-wikilink-label" });

function buildWikilinkDecorations(view) {
  const builder = new RangeSetBuilder();
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.doc.sliceString(from, to);
    WIKILINK_RE.lastIndex = 0;
    let m;
    while ((m = WIKILINK_RE.exec(text))) {
      const matchStart = from + m.index;
      const matchEnd = matchStart + m[0].length;
      const targetStart = matchStart + 2;
      const targetEnd = targetStart + m[1].length;
      builder.add(matchStart, targetStart, bracketMark); // [[
      builder.add(targetStart, targetEnd, targetMark);
      if (m[2] !== undefined) {
        const pipeStart = targetEnd;
        const pipeEnd = pipeStart + 1;
        const labelStart = pipeEnd;
        const labelEnd = labelStart + m[2].length;
        builder.add(pipeStart, pipeEnd, bracketMark);
        builder.add(labelStart, labelEnd, labelMark);
      }
      builder.add(matchEnd - 2, matchEnd, bracketMark); // ]]
    }
  }
  return builder.finish();
}

const wikilinkHighlighter = ViewPlugin.fromClass(
  class {
    constructor(view) {
      this.decorations = buildWikilinkDecorations(view);
    }
    update(u) {
      if (u.docChanged || u.viewportChanged) {
        this.decorations = buildWikilinkDecorations(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

// ---------------------------------------------------------------------------
// Wikilink autocomplete — port of editor-wikilink-complete.ts.
//
// The search index in docgen-rs ships fields { slug, title, text }. The
// original code expected { id, title, path }. We adapt by mapping `slug` to
// both the `id` (the inserted target) and the displayed `path`.
// ---------------------------------------------------------------------------

let entriesCache = null;
let entriesPromise = null;
let searchIndexBase = "";

async function loadEntries() {
  if (entriesCache) return entriesCache;
  if (!entriesPromise) {
    entriesPromise = fetch(`${searchIndexBase}/search-index.json`)
      .then((r) => r.json())
      .then((arr) => {
        const out = arr.map((d) => {
          // Field adaptation: docgen-rs uses `slug`; original used `id`/`path`.
          const id = d.id ?? d.slug ?? "";
          const path = d.path ?? d.slug ?? id;
          const title = d.title ?? id;
          return {
            id,
            title,
            path,
            titleLower: String(title).toLowerCase(),
            idLower: String(id).toLowerCase(),
            pathLower: String(path).toLowerCase(),
          };
        });
        entriesCache = out;
        return out;
      });
  }
  return entriesPromise;
}

function fuzzyScan(query, target) {
  if (!query) return { score: 0, indices: [] };
  const indices = [];
  let q = 0;
  let lastPos = -2;
  let run = 0;
  let score = 0;
  for (let i = 0; i < target.length && q < query.length; i++) {
    if (target[i] === query[q]) {
      indices.push(i);
      run = i === lastPos + 1 ? run + 1 : 1;
      const prev = i > 0 ? target[i - 1] : "/";
      if (/[\\/\-_ .]/.test(prev)) score += 6;
      if (i === 0) score += 8;
      score += run * 2;
      lastPos = i;
      q++;
    }
  }
  if (q < query.length) return null;
  score -= target.length * 0.04;
  return { score, indices };
}

function matchEntry(query, entry) {
  if (!query) {
    return { score: 0, titleIndices: [], pathIndices: [] };
  }
  const q = query.toLowerCase();
  const titleMatch = fuzzyScan(q, entry.titleLower);
  const pathMatch = fuzzyScan(q, entry.idLower) ?? fuzzyScan(q, entry.pathLower);
  if (!titleMatch && !pathMatch) return null;
  const titleScore = titleMatch ? titleMatch.score + 5 : -Infinity;
  const pathScore = pathMatch ? pathMatch.score : -Infinity;
  const best = Math.max(titleScore, pathScore);
  return {
    score: best,
    titleIndices: titleMatch ? titleMatch.indices : [],
    pathIndices: pathMatch ? pathMatch.indices : [],
  };
}

function renderHighlighted(text, indices, extraClass) {
  const wrapper = document.createElement("span");
  wrapper.className = extraClass;
  if (!indices.length) {
    wrapper.textContent = text;
    return wrapper;
  }
  let cursor = 0;
  for (const idx of indices) {
    if (idx > cursor) {
      wrapper.appendChild(document.createTextNode(text.slice(cursor, idx)));
    }
    const mark = document.createElement("span");
    mark.className = "cm-wikilink-suggestion__match";
    mark.textContent = text[idx];
    wrapper.appendChild(mark);
    cursor = idx + 1;
  }
  if (cursor < text.length) {
    wrapper.appendChild(document.createTextNode(text.slice(cursor)));
  }
  return wrapper;
}

function buildCompletion(entry, match, from, to) {
  return {
    label: entry.title,
    boost: match.score,
    apply(view, _completion, applyFrom, applyTo) {
      const state = view.state;
      let end = applyTo;
      // Swallow auto-closed `]]` (or single `]`) if it follows the range.
      const tail = state.doc.sliceString(end, Math.min(end + 2, state.doc.length));
      if (tail.startsWith("]]")) end += 2;
      else if (tail.startsWith("]")) end += 1;
      const insert = `${entry.id}|${entry.title}]]`;
      view.dispatch({
        changes: { from: applyFrom, to: end, insert },
        selection: { anchor: applyFrom + insert.length },
      });
    },
    info() {
      const el = document.createElement("div");
      el.className = "cm-wikilink-suggestion__info";
      el.textContent = entry.path;
      return el;
    },
    _wikilink: { entry, match, from, to },
  };
}

async function wikilinkSource(context) {
  const line = context.state.doc.lineAt(context.pos);
  const beforeCursor = line.text.slice(0, context.pos - line.from);
  const openMatch = /\[\[([^\]\n|]*)$/.exec(beforeCursor);
  if (!openMatch) return null;
  const queryRaw = openMatch[1];
  const queryStart = context.pos - queryRaw.length;
  const queryEnd = context.pos;
  const entries = await loadEntries();
  const scored = [];
  for (const entry of entries) {
    const match = matchEntry(queryRaw, entry);
    if (match) scored.push({ entry, match });
  }
  scored.sort((a, b) => b.match.score - a.match.score);
  const minScore = queryRaw.length >= 2 ? queryRaw.length * 3 : -Infinity;
  const filtered = queryRaw
    ? scored.filter((s) => s.match.score >= minScore)
    : scored;
  const top = filtered.slice(0, 30);
  return {
    from: queryStart,
    to: queryEnd,
    filter: false,
    options: top.map((s) =>
      buildCompletion(s.entry, s.match, queryStart, queryEnd),
    ),
  };
}

function renderWikilinkOption(completion) {
  const data = completion._wikilink;
  if (!data) return null;
  const wrap = document.createElement("div");
  wrap.className = "cm-wikilink-suggestion";
  wrap.appendChild(
    renderHighlighted(
      data.entry.title,
      data.match.titleIndices,
      "cm-wikilink-suggestion__title",
    ),
  );
  wrap.appendChild(
    renderHighlighted(
      data.entry.id,
      data.match.pathIndices,
      "cm-wikilink-suggestion__path",
    ),
  );
  return wrap;
}

const wikilinkAutocompletion = autocompletion({
  override: [wikilinkSource],
  activateOnTyping: true,
  closeOnBlur: true,
  maxRenderedOptions: 30,
  defaultKeymap: true,
  icons: false,
  addToOptions: [{ render: renderWikilinkOption, position: 20 }],
});

// ---------------------------------------------------------------------------
// Table formatting — port of editor-format-table.ts.
// ---------------------------------------------------------------------------

function splitCells(inner) {
  const cells = [];
  let buf = "";
  let wikilinkDepth = 0;
  let inCode = false;
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (c === "\\" && inner[i + 1] === "|") {
      buf += "\\|";
      i++;
      continue;
    }
    if (!inCode && c === "[" && inner[i + 1] === "[") {
      wikilinkDepth++;
      buf += "[[";
      i++;
      continue;
    }
    if (!inCode && c === "]" && inner[i + 1] === "]" && wikilinkDepth > 0) {
      wikilinkDepth--;
      buf += "]]";
      i++;
      continue;
    }
    if (c === "`" && wikilinkDepth === 0) {
      inCode = !inCode;
      buf += c;
      continue;
    }
    if (c === "|" && wikilinkDepth === 0 && !inCode) {
      cells.push(buf);
      buf = "";
      continue;
    }
    buf += c;
  }
  cells.push(buf);
  return cells;
}

function isSeparatorCell(cell) {
  return /^:?-{1,}:?$/.test(cell.trim());
}

function detectAlign(cell) {
  const c = cell.trim();
  const left = c.startsWith(":");
  const right = c.endsWith(":");
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return "none";
}

function isTableLine(line) {
  const trimmed = line.trim();
  return trimmed.startsWith("|") && trimmed.endsWith("|") && trimmed.length >= 2;
}

function stripFence(text) {
  const tx = text.trim();
  return tx.slice(1, -1);
}

function padCell(text, width, align) {
  const need = width - text.length;
  if (need <= 0) return text;
  if (align === "right") return " ".repeat(need) + text;
  if (align === "center") {
    const left = Math.floor(need / 2);
    return " ".repeat(left) + text + " ".repeat(need - left);
  }
  return text + " ".repeat(need);
}

function buildSeparatorCell(width, align) {
  const w = Math.max(width, align === "center" ? 4 : align === "none" ? 3 : 3);
  if (align === "center") return ":" + "-".repeat(w - 2) + ":";
  if (align === "left") return ":" + "-".repeat(w - 1);
  if (align === "right") return "-".repeat(w - 1) + ":";
  return "-".repeat(w);
}

const formatTableAtCursor = (view) => {
  const state = view.state;
  const cursorPos = state.selection.main.head;
  const cursorLineNo = state.doc.lineAt(cursorPos).number;

  if (!isTableLine(state.doc.line(cursorLineNo).text)) return false;

  let startLine = cursorLineNo;
  let endLine = cursorLineNo;
  while (startLine > 1 && isTableLine(state.doc.line(startLine - 1).text))
    startLine--;
  while (endLine < state.doc.lines && isTableLine(state.doc.line(endLine + 1).text))
    endLine++;
  if (endLine - startLine < 1) return false;

  const rawRows = [];
  for (let n = startLine; n <= endLine; n++) {
    const text = stripFence(state.doc.line(n).text);
    rawRows.push(splitCells(text).map((c) => c.trim().replace(/\s+/g, " ")));
  }

  let sepIdx = -1;
  for (let i = 0; i < rawRows.length; i++) {
    if (rawRows[i].length > 0 && rawRows[i].every(isSeparatorCell)) {
      sepIdx = i;
      break;
    }
  }
  if (sepIdx < 1) return false;

  const sepRow = rawRows[sepIdx];
  const aligns = sepRow.map(detectAlign);
  const colCount = Math.max(...rawRows.map((r) => r.length));
  for (const r of rawRows) while (r.length < colCount) r.push("");
  while (aligns.length < colCount) aligns.push("none");

  const widths = new Array(colCount).fill(0);
  for (let i = 0; i < rawRows.length; i++) {
    if (i === sepIdx) continue;
    rawRows[i].forEach((c, j) => {
      if (c.length > widths[j]) widths[j] = c.length;
    });
  }
  for (let j = 0; j < colCount; j++) {
    const minSep = aligns[j] === "center" ? 4 : 3;
    if (widths[j] < minSep) widths[j] = minSep;
  }

  const outLines = [];
  for (let i = 0; i < rawRows.length; i++) {
    if (i === sepIdx) {
      const sepCells = widths.map((w, j) => buildSeparatorCell(w, aligns[j]));
      outLines.push("| " + sepCells.join(" | ") + " |");
    } else {
      const cells = rawRows[i].map((c, j) => padCell(c, widths[j], aligns[j]));
      outLines.push("| " + cells.join(" | ") + " |");
    }
  }

  const from = state.doc.line(startLine).from;
  const to = state.doc.line(endLine).to;
  const insert = outLines.join("\n");
  if (insert === state.doc.sliceString(from, to)) return true;

  view.dispatch({ changes: { from, to, insert }, userEvent: "format.table" });
  return true;
};

const formatTableKeymap = [
  { key: "Mod-Alt-l", run: formatTableAtCursor, preventDefault: true },
];

// ---------------------------------------------------------------------------
// Small DOM helpers.
// ---------------------------------------------------------------------------

function el(tag, props, children) {
  const node = document.createElement(tag);
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (v == null) continue;
      if (k === "class") node.className = v;
      else if (k === "text") node.textContent = v;
      else if (k.startsWith("on") && typeof v === "function") {
        node.addEventListener(k.slice(2).toLowerCase(), v);
      } else if (k === "href" || k === "type" || k === "role" || k === "aria-label") {
        node.setAttribute(k, v);
      } else {
        node.setAttribute(k, v);
      }
    }
  }
  if (children) {
    for (const c of children) {
      if (c == null) continue;
      node.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
    }
  }
  return node;
}

// Inline SVG for the "table" icon (port of the Icon name="table").
function tableIcon() {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "18");
  svg.setAttribute("height", "18");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.6");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  const paths = [
    "M3 5h18v14H3z",
    "M3 10h18",
    "M3 15h18",
    "M9 5v14",
    "M15 5v14",
  ];
  for (const d of paths) {
    const p = document.createElementNS(ns, "path");
    p.setAttribute("d", d);
    svg.appendChild(p);
  }
  return svg;
}

// Build a stroked SVG icon from a list of <path d=...> values.
function svgIcon(paths, size) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of paths) {
    const p = document.createElementNS(ns, "path");
    p.setAttribute("d", d);
    svg.appendChild(p);
  }
  return svg;
}

function menuIcon() {
  return svgIcon(["M4 7h16", "M4 12h16", "M4 17h16"], 18);
}

function checkIcon() {
  return svgIcon(["M20 6L9 17l-5-5"], 12);
}

function isMac() {
  return /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
}

// ---------------------------------------------------------------------------
// Editor app.
// ---------------------------------------------------------------------------

const WRAP_KEY = "doc-editor-wrap";

function readWrapPref() {
  try {
    const v = localStorage.getItem(WRAP_KEY);
    return v === null ? true : v === "true";
  } catch {
    return true;
  }
}

function writeWrapPref(v) {
  try {
    localStorage.setItem(WRAP_KEY, String(v));
  } catch {
    /* ignore */
  }
}

const THEME_KEY = "doc-editor-theme";

function readThemePref() {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return v && EDITOR_THEMES.some((th) => th.id === v) ? v : "default";
  } catch {
    return "default";
  }
}

function writeThemePref(v) {
  try {
    localStorage.setItem(THEME_KEY, v);
  } catch {
    /* ignore */
  }
}

function mountEditor(root) {
  const docPath = root.dataset.docPath || "";
  const viewPath = root.dataset.viewPath || "";
  const title = root.dataset.title || docPath;
  const base = root.dataset.base || "";
  searchIndexBase = base;

  // --- State -------------------------------------------------------------
  let buffer = "";
  let lastSavedSource = "";
  let diskHash = "";
  let headSource = "";
  let saving = false;
  let loading = true;
  let statusMessage = "Loading source...";
  let errorMessage = null;
  let previewTimer = null;
  let view = null;

  const dirty = () => buffer !== lastSavedSource;

  // --- DOM scaffold ------------------------------------------------------
  root.textContent = "";
  const article = el("article", { class: "doc-editor" });

  // Header
  const statusSpan = el("span");
  const saveBtn = el("button", {
    type: "button",
    class: "is-primary",
    onclick: () => void save(),
  });
  const formatBtn = el("button", {
    type: "button",
    class: "icon-only icon-action",
    "aria-label": "Format table",
    onclick: () => formatTable(),
  });
  formatBtn.appendChild(tableIcon());
  const tooltip = el("span", { class: "tooltip", role: "tooltip" }, [
    el("span", { class: "tooltip__name", text: "Format table" }),
    el("kbd", {
      class: "tooltip__kbd",
      text: isMac() ? "⌘⌥L" : "Ctrl+Alt+L",
    }),
  ]);
  formatBtn.appendChild(tooltip);

  const viewLink = el("a", { href: `${base}${viewPath}`, text: "View" });

  // --- Settings menu: word wrap + 8 editor themes (persisted) -----------
  let activeTheme = readThemePref();
  // Hoisted so the menu (built here) can call it before themeCompartment is
  // initialized in createEditor() — it's only ever *invoked* after mount.
  function setTheme(id) {
    activeTheme = id;
    writeThemePref(id);
    if (view) {
      view.dispatch({
        effects: themeCompartment.reconfigure(buildEditorTheme(id)),
      });
    }
  }

  const settingsHost = el("div", { class: "settings-menu" });
  const settingsBtn = el("button", {
    type: "button",
    class: "icon-only icon-btn",
    "aria-haspopup": "menu",
    "aria-expanded": "false",
    "aria-label": "Editor settings",
    title: "Editor settings",
  });
  settingsBtn.appendChild(menuIcon());
  let settingsOpen = false;
  let settingsPanel = null;

  function renderThemeChecks() {
    if (!settingsPanel) return;
    settingsPanel.querySelectorAll("[data-theme-id]").forEach((b) => {
      const on = b.getAttribute("data-theme-id") === activeTheme;
      b.classList.toggle("is-active", on);
      b.setAttribute("aria-checked", String(on));
      const check = b.querySelector(".check");
      if (check) check.style.visibility = on ? "visible" : "hidden";
    });
  }

  function buildSettingsPanel() {
    const panel = el("div", {
      class: "panel",
      role: "menu",
      "aria-label": "Editor settings",
    });
    panel.appendChild(el("div", { class: "panel__label", text: "Options" }));
    const wrapInput = el("input", { type: "checkbox" });
    wrapInput.checked = readWrapPref();
    wrapInput.addEventListener("change", () => {
      // _docgenToggleWrap flips + persists + reconfigures; only call it if the
      // checkbox actually diverged from the stored pref.
      if (readWrapPref() !== wrapInput.checked) root._docgenToggleWrap();
    });
    panel.appendChild(
      el("label", { class: "toggle-row" }, [
        wrapInput,
        el("span", { text: "Word wrap" }),
      ]),
    );
    panel.appendChild(el("div", { class: "panel__divider" }));
    panel.appendChild(
      el("div", { class: "panel__label", text: "Editor theme" }),
    );
    const list = el("ul", { class: "theme-list" });
    for (const th of EDITOR_THEMES) {
      const swatch = el("span", { class: "swatch", "aria-hidden": "true" });
      swatch.style.background = th.swatch;
      const check = el("span", { class: "check", "aria-hidden": "true" });
      check.appendChild(checkIcon());
      const item = el(
        "button",
        {
          type: "button",
          class: "theme-item",
          role: "menuitemradio",
          "data-theme-id": th.id,
        },
        [swatch, el("span", { class: "name", text: th.label }), check],
      );
      item.addEventListener("click", () => {
        setTheme(th.id);
        renderThemeChecks();
        closeSettings();
      });
      list.appendChild(el("li", null, [item]));
    }
    panel.appendChild(list);
    return panel;
  }

  function openSettings() {
    if (settingsOpen) return;
    settingsOpen = true;
    settingsPanel = buildSettingsPanel();
    settingsHost.appendChild(settingsPanel);
    settingsBtn.classList.add("is-active");
    settingsBtn.setAttribute("aria-expanded", "true");
    renderThemeChecks();
  }
  function closeSettings() {
    if (!settingsOpen) return;
    settingsOpen = false;
    if (settingsPanel) {
      settingsPanel.remove();
      settingsPanel = null;
    }
    settingsBtn.classList.remove("is-active");
    settingsBtn.setAttribute("aria-expanded", "false");
  }
  settingsBtn.addEventListener("click", () =>
    settingsOpen ? closeSettings() : openSettings(),
  );
  document.addEventListener("mousedown", (e) => {
    if (settingsOpen && !settingsHost.contains(e.target)) closeSettings();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && settingsOpen) closeSettings();
  });
  settingsHost.appendChild(settingsBtn);

  const header = el("header", { class: "editor-header" }, [
    el("div", null, [
      el("p", { class: "eyebrow", text: "Editor" }),
      el("h1", { text: title }),
      el("p", { class: "path", text: docPath }),
    ]),
    el("div", { class: "editor-actions" }, [
      statusSpan,
      el("div", { class: "btn-strip" }, [formatBtn]),
      settingsHost,
      el("div", { class: "btn-strip" }, [viewLink, saveBtn]),
    ]),
  ]);

  // Error banner (created lazily)
  let errorBanner = null;

  // Workspace
  const sourceHost = el("div", { class: "source-editor" });
  const sourcePane = el("div", { class: "pane source-pane" }, [sourceHost]);

  // The preview is rendered server-side through the SAME pipeline as a published
  // page and shown in an <iframe>, so mermaid diagrams, custom components, and
  // wikilink tooltips hydrate with the real island stack — identical to the
  // built site. (innerHTML injection can't re-run the per-island bootstrap.)
  const previewFrame = el("iframe", { class: "doc-preview-frame", title: "Preview" });
  previewFrame.srcdoc =
    '<!DOCTYPE html><html><body style="margin:0;padding:28px;font:13px system-ui;color:#888;background:#111">Preparing preview…</body></html>';
  const previewPane = el("div", { class: "pane preview-pane" }, [previewFrame]);

  const workspace = el("section", { class: "editor-workspace" }, [
    sourcePane,
    previewPane,
  ]);

  article.appendChild(header);
  article.appendChild(workspace);
  root.appendChild(article);

  // --- Status / render helpers ------------------------------------------
  function renderStatus() {
    statusSpan.textContent = errorMessage ?? statusMessage;
    statusSpan.classList.toggle("dirty", dirty());
    statusSpan.classList.toggle("error", Boolean(errorMessage));
    saveBtn.textContent = saving ? "Saving..." : "Save";
    saveBtn.disabled = !dirty() || saving || loading;
  }

  function renderErrorBanner() {
    if (errorMessage) {
      if (!errorBanner) {
        errorBanner = el("section", { class: "editor-error" });
        article.insertBefore(errorBanner, workspace);
      }
      errorBanner.textContent = errorMessage;
    } else if (errorBanner) {
      errorBanner.remove();
      errorBanner = null;
    }
  }

  function setPreviewHtml(html) {
    // Preserve the reader's scroll position across the srcdoc swap (the iframe is
    // same-origin, so we can read/restore its scroll) — otherwise every debounced
    // re-render would jump a long doc back to the top.
    let prevScroll = 0;
    try {
      prevScroll = previewFrame.contentWindow ? previewFrame.contentWindow.scrollY : 0;
    } catch {
      /* cross-origin guard; ignore */
    }
    previewFrame.onload = () => {
      try {
        previewFrame.contentWindow.scrollTo(0, prevScroll);
      } catch {
        /* ignore */
      }
    };
    previewFrame.srcdoc = html; // trusted server output (full content-only document)
  }

  function setPreviewError(message) {
    previewFrame.onload = null;
    const esc = (s) =>
      String(s)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
    previewFrame.srcdoc = `<!DOCTYPE html><html><head><meta charset="utf-8"><link rel="stylesheet" href="${base}/docgen.css"></head><body class="docgen-app" style="margin:0;padding:24px"><section class="preview-error" aria-live="polite"><strong>Preview failed</strong><pre>${esc(message)}</pre></section></body></html>`;
  }

  // --- Editor construction ----------------------------------------------
  const wrapCompartment = new Compartment();
  const themeCompartment = new Compartment();

  function createEditor() {
    if (view) return;
    const initialWrap = readWrapPref() ? EditorView.lineWrapping : [];
    view = new EditorView({
      parent: sourceHost,
      doc: buffer,
      extensions: [
        basicSetup,
        markdown(),
        unifiedMergeView({
          original: headSource,
          gutter: true,
          highlightChanges: true,
          mergeControls: false,
        }),
        themeCompartment.of(buildEditorTheme(readThemePref())),
        wikilinkHighlighter,
        wrapCompartment.of(initialWrap),
        wikilinkAutocompletion,
        Prec.high(keymap.of(formatTableKeymap)),
        Prec.high(
          keymap.of([
            {
              key: "Tab",
              run: (v) => {
                if (completionStatus(v.state) !== "active") return false;
                return acceptCompletion(v);
              },
            },
            {
              key: "Mod-s",
              run: () => {
                void save();
                return true;
              },
              preventDefault: true,
            },
          ]),
        ),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          onEditorChange(update.state.doc.toString());
        }),
        EditorView.domEventHandlers({
          keydown(event) {
            if (
              (event.metaKey || event.ctrlKey) &&
              event.key.toLowerCase() === "s"
            ) {
              event.preventDefault();
              void save();
              return true;
            }
            return false;
          },
        }),
      ],
    });
  }

  function formatTable() {
    if (!view) return false;
    const result = formatTableAtCursor(view);
    view.focus();
    return result;
  }

  // --- Behavior ----------------------------------------------------------
  function onEditorChange(next) {
    buffer = next;
    statusMessage = next === lastSavedSource ? "Saved" : "Unsaved changes";
    renderStatus();
    schedulePreview(next);
  }

  function schedulePreview(source) {
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(() => void updatePreview(source), 350);
  }

  async function updatePreview(source) {
    try {
      const res = await fetch(`${base}/__docgen/preview`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: docPath, source }),
      });
      if (!res.ok) throw new Error(`Preview failed (${res.status})`);
      const payload = await res.json();
      setPreviewHtml(payload.html);
    } catch (error) {
      setPreviewError(error instanceof Error ? error.message : String(error));
    }
  }

  async function loadSource() {
    loading = true;
    errorMessage = null;
    statusMessage = "Loading source...";
    renderStatus();
    renderErrorBanner();
    try {
      const res = await fetch(
        `${base}/__docgen/source?path=${encodeURIComponent(docPath)}`,
      );
      if (!res.ok) throw new Error(`Load failed (${res.status})`);
      const payload = await res.json();
      buffer = payload.source ?? "";
      lastSavedSource = buffer;
      diskHash = payload.disk_hash ?? "";
      headSource = payload.head_source ?? "";
      statusMessage = "Loaded";
      createEditor();
      void updatePreview(buffer);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      statusMessage = "Load failed";
    } finally {
      loading = false;
      renderStatus();
      renderErrorBanner();
    }
  }

  async function save() {
    if (loading || saving || !dirty()) return;
    saving = true;
    errorMessage = null;
    statusMessage = "Saving...";
    renderStatus();
    renderErrorBanner();
    try {
      const res = await fetch(`${base}/__docgen/source`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: docPath, source: buffer, disk_hash: diskHash }),
      });
      if (res.status === 409) {
        throw new Error("source changed on disk");
      }
      if (!res.ok) throw new Error(`Save failed (${res.status})`);
      const saved = await res.json();
      lastSavedSource = buffer;
      diskHash = saved.disk_hash ?? diskHash;
      statusMessage = `Saved ${new Date().toLocaleTimeString()}`;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      statusMessage = "Save failed";
    } finally {
      saving = false;
      renderStatus();
      renderErrorBanner();
    }
  }

  // --- Wrap toggle (optional nicety) ------------------------------------
  // Exposed for potential external UI; toggles line wrapping + persists.
  root._docgenToggleWrap = function toggleWrap() {
    const next = !readWrapPref();
    writeWrapPref(next);
    if (view) {
      view.dispatch({
        effects: wrapCompartment.reconfigure(
          next ? EditorView.lineWrapping : [],
        ),
      });
    }
    return next;
  };

  // --- beforeunload guard ------------------------------------------------
  function onBeforeUnload(event) {
    if (!dirty()) return;
    event.preventDefault();
    event.returnValue = "";
  }
  window.addEventListener("beforeunload", onBeforeUnload);

  // Kick off.
  renderStatus();
  void loadSource();
}

// ---------------------------------------------------------------------------
// Auto-init.
// ---------------------------------------------------------------------------

function init() {
  const root = document.getElementById("docgen-editor-app");
  if (root) mountEditor(root);
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init, { once: true });
} else {
  init();
}
