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

  const header = el("header", { class: "editor-header" }, [
    el("div", null, [
      el("p", { class: "eyebrow", text: "Editor" }),
      el("h1", { text: title }),
      el("p", { class: "path", text: docPath }),
    ]),
    el("div", { class: "editor-actions" }, [
      statusSpan,
      el("div", { class: "btn-strip" }, [formatBtn]),
      el("div", { class: "btn-strip" }, [viewLink, saveBtn]),
    ]),
  ]);

  // Error banner (created lazily)
  let errorBanner = null;

  // Workspace
  const sourceHost = el("div", { class: "source-editor" });
  const sourcePane = el("div", { class: "pane source-pane" }, [sourceHost]);

  const previewContent = el("section", { class: "doc-content" }, [
    el("p", { class: "preview-muted", text: "Preparing preview..." }),
  ]);
  const previewArticle = el("article", { class: "doc-editor-preview" }, [
    el("header", { class: "doc-header" }, [
      el("p", { class: "eyebrow", text: "Preview" }),
      el("h1", { text: title }),
    ]),
    previewContent,
  ]);
  const previewPane = el("div", { class: "pane preview-pane" }, [
    previewArticle,
  ]);

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
    previewContent.innerHTML = html; // trusted server output
  }

  function setPreviewError(message) {
    previewContent.textContent = "";
    const errSection = el("section", {
      class: "preview-error",
      "aria-live": "polite",
    }, [
      el("strong", { text: "Preview failed" }),
      el("pre", { text: message }),
    ]);
    previewContent.appendChild(errSection);
  }

  // --- Editor construction ----------------------------------------------
  const wrapCompartment = new Compartment();

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
        buildVarTheme(),
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
