//! Static HTML rendering of a base's views. Produces a self-contained
//! `<div class="docgen-base">` styled by the shared `docgen.css`. Each view
//! (table/cards/list) is filtered, sorted, grouped, and limited exactly as
//! configured, then rendered to server-side HTML.
//!
//! When [`RenderOptions::interactive`] is set the same HTML additionally carries
//! `data-*` hooks and a per-view JSON payload (see [`crate::interactive`]) that the
//! client island progressively enhances into an interactive view; the payload is
//! keys-only, so the server HTML stays the single source of what each cell looks
//! like. With `interactive` false the output is exactly the pure static HTML.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::Expr;
use crate::eval::EvalCtx;
use crate::filter;
use crate::interactive::{self, RowView};
use crate::model::{BaseFile, GroupBy, View};
use crate::note::{Corpus, Note};
use crate::parser::parse;
use crate::semver;
use crate::value::Value;

/// Rendering configuration supplied by the host (docgen).
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Site base path (e.g. `/docs` or empty) prefixed onto internal note URLs.
    pub base: String,
    /// Fallback title used for a view with no `name`.
    pub default_view_name: String,
    /// When true, emit the interactive DOM hooks + JSON payload the client-side
    /// island hydrates against. When false, output is byte-for-byte the pure
    /// static HTML (no `data-*` hooks, no payload script).
    pub interactive: bool,
    /// Which base on the page this is: 0 for a standalone `.base`, else the index
    /// of the ` ```base ` block. Views number from 0 within their own base, so the
    /// emitted `data-base-view` is `{block_index}-{view_index}` — the island keys
    /// URL-hash segments and facet-panel DOM ids off that string, and two blocks
    /// on one page would otherwise both claim `0` and clobber each other.
    pub block_index: usize,
}

impl RenderOptions {
    fn note_url(&self, slug: &str) -> String {
        format!("{}/{}", self.base.trim_end_matches('/'), slug)
    }
}

/// Render every view in a base to HTML. Compiles formulas + the global filter
/// once and reuses them across views.
pub fn render_base(base: &BaseFile, corpus: &Corpus, opts: &RenderOptions) -> String {
    // Pre-parse formulas (name → expr); an unparsable formula becomes an
    // always-null expression so cells render empty rather than failing.
    let formulas = parse_formulas(&base.formulas);
    let custom_summaries = parse_formulas(&base.summaries);

    let mut out = String::from("<div class=\"docgen-base\">");
    if base.views.is_empty() {
        // A base with no views still shows an (empty) default table.
        let default = View {
            view_type: "table".into(),
            ..Default::default()
        };
        out.push_str(&render_view(
            &default,
            0,
            base,
            corpus,
            opts,
            &formulas,
            &custom_summaries,
        ));
    }
    for (idx, view) in base.views.iter().enumerate() {
        out.push_str(&render_view(
            view,
            idx,
            base,
            corpus,
            opts,
            &formulas,
            &custom_summaries,
        ));
    }
    out.push_str("</div>");
    out
}

/// Parse a base's malformed YAML gracefully: on a parse error, return a styled
/// error block (never a panic), mirroring the PlantUML error-component ethos.
pub fn render_base_source(yaml: &str, corpus: &Corpus, opts: &RenderOptions) -> String {
    match crate::model::parse_base(yaml) {
        Ok(base) => render_base(&base, corpus, opts),
        Err(e) => error_block(&format!("could not parse .base YAML: {e}")),
    }
}

fn parse_formulas(src: &BTreeMap<String, String>) -> BTreeMap<String, Expr> {
    src.iter()
        .map(|(k, v)| (k.clone(), parse(v).unwrap_or(Expr::Null)))
        .collect()
}

/// A row: the note plus its pre-evaluated column values (keyed by column ref).
/// `id` is the stable index into the post-sort row slice (assigned before
/// grouping), joining SSR DOM nodes to their payload entry in interactive mode.
struct Row<'a> {
    id: usize,
    note: &'a Note,
    cells: BTreeMap<String, Value>,
}

#[allow(clippy::too_many_arguments)]
fn render_view(
    view: &View,
    view_index: usize,
    base: &BaseFile,
    corpus: &Corpus,
    opts: &RenderOptions,
    formulas: &BTreeMap<String, Expr>,
    custom_summaries: &BTreeMap<String, Expr>,
) -> String {
    // Honor the per-base/per-view `docgenInteractive:false` opt-out end to end:
    // a view the author disabled renders as pure static HTML even when the build
    // requested interactive output. Shadow `opts` so every downstream renderer
    // (section hooks, body `data-*`, payload) sees the effective flag.
    let interactive = opts.interactive && interactive::view_interactive_enabled(base, view);
    let eff_opts_owned;
    let opts: &RenderOptions = if interactive == opts.interactive {
        opts
    } else {
        eff_opts_owned = RenderOptions {
            interactive: false,
            ..opts.clone()
        };
        &eff_opts_owned
    };

    let predicate = filter::combine(&base.filters, &view.filters);

    // Collect matching notes.
    let matching: Vec<&Note> = corpus
        .notes
        .iter()
        .filter(|n| {
            let ctx = EvalCtx::new(n, corpus, formulas);
            predicate.matches(&ctx)
        })
        .collect();

    // Determine the columns to display.
    let columns = resolve_columns(view, &matching, base);
    let column_exprs: Vec<(String, Expr)> = columns
        .iter()
        .map(|c| (c.clone(), parse(c).unwrap_or(Expr::Null)))
        .collect();

    // Evaluate each row's cells.
    let mut rows: Vec<Row> = matching
        .iter()
        .map(|n| {
            let ctx = EvalCtx::new(n, corpus, formulas);
            let cells = column_exprs
                .iter()
                .map(|(key, expr)| (key.clone(), ctx.eval(expr)))
                .collect();
            Row {
                id: 0,
                note: n,
                cells,
            }
        })
        .collect();

    // Sort by the view's sort keys (evaluated on the fly), stable, in order.
    apply_sort(&mut rows, view, corpus, formulas);

    // Assign each row a stable id = its index in the post-sort slice, BEFORE
    // grouping, so the payload and the DOM `data-row` attributes agree.
    for (i, row) in rows.iter_mut().enumerate() {
        row.id = i;
    }

    // `limit` truncates the row set, in interactive mode too — it means the same
    // thing Obsidian means by it, and the same thing this renderer's static mode
    // has always meant. Pagination is `docgenInteractive.pageSize`'s job and only
    // its job; conflating the two made `limit: 10` ship every matched row to the
    // client and page them 10 at a time, so a cap the author wrote was not a cap
    // at all. Truncating HERE (once, before both the payload and the body) is what
    // keeps them agreeing: a row the DOM does not contain must not appear in the
    // payload, or the island would hold ids with no node. Applies across the whole
    // view, not per group.
    if let Some(limit) = view.limit {
        rows.truncate(limit);
    }

    // Group (optional).
    let name = view
        .name
        .clone()
        .unwrap_or_else(|| opts.default_view_name.clone());

    let body = match view.view_type.as_str() {
        "cards" => render_cards(&rows, base, &columns, opts, corpus),
        "list" => render_list(&rows, &columns, opts, corpus, formulas),
        // "table" and any unknown type fall back to a table.
        _ => render_table(
            &rows,
            view,
            base,
            &columns,
            opts,
            corpus,
            formulas,
            custom_summaries,
        ),
    };

    let mut section = String::new();
    if opts.interactive {
        section.push_str(&format!(
            "<section class=\"docgen-base-view\" data-base-view=\"{}-{view_index}\">",
            opts.block_index
        ));
    } else {
        section.push_str("<section class=\"docgen-base-view\">");
    }
    if !name.is_empty() {
        section.push_str(&format!(
            "<div class=\"docgen-base-view__title\">{}</div>",
            escape(&name)
        ));
    }
    // Surface filter parse errors as a small diagnostic (non-fatal).
    let mut errs = Vec::new();
    predicate.errors(&mut errs);
    if !errs.is_empty() {
        section.push_str(&format!(
            "<div class=\"docgen-base-warning\">Filter parse error: {}</div>",
            escape(&errs.join("; "))
        ));
    }
    // A typo'd key in docgen's own `docgenInteractive` block would otherwise do
    // nothing, forever, while looking like it worked. Warn visibly but keep
    // rendering: the build always succeeds (the graceful-degradation contract),
    // and only the mistyped knob is lost, not the view.
    if let Some(warning) = view
        .interactive
        .as_ref()
        .and_then(|iv| iv.unknown_key_warning())
    {
        section.push_str(&format!(
            "<div class=\"docgen-base-warning\">{}</div>",
            escape(&warning)
        ));
    }
    // Interactive payload: after the optional title/warning, before the body.
    if opts.interactive {
        let row_views: Vec<RowView> = rows
            .iter()
            .map(|r| RowView {
                id: r.id,
                cells: &r.cells,
            })
            .collect();
        let json = interactive::build_payload(view, base, &columns, &row_views, opts);
        section.push_str(&format!(
            "<script type=\"application/json\" class=\"docgen-base-data\">{json}</script>"
        ));
    }
    section.push_str(&body);
    section.push_str("</section>");
    section
}

/// Choose the display columns: the view's `order` if given, else `file.name` plus
/// every note property key seen (deterministic, sorted).
fn resolve_columns(view: &View, matching: &[&Note], _base: &BaseFile) -> Vec<String> {
    if !view.order.is_empty() {
        return view.order.clone();
    }
    let mut props: BTreeSet<String> = BTreeSet::new();
    for n in matching {
        for k in n.properties.keys() {
            props.insert(k.clone());
        }
    }
    let mut cols = vec!["file.name".to_string()];
    cols.extend(props.into_iter().map(|p| format!("note.{p}")));
    cols
}

fn apply_sort(
    rows: &mut Vec<Row>,
    view: &View,
    corpus: &Corpus,
    formulas: &BTreeMap<String, Expr>,
) {
    if view.sort.is_empty() {
        return;
    }
    let keys: Vec<(Expr, bool)> = view
        .sort
        .iter()
        .map(|k| (parse(k.property()).unwrap_or(Expr::Null), k.descending()))
        .collect();
    // Evaluate each row's sort keys ONCE (a decorate-sort), rather than
    // re-evaluating inside every pairwise comparison — comparisons are O(n log n),
    // and a `formula.*` sort key can be expensive.
    let mut decorated: Vec<(Vec<Value>, Row)> = rows
        .drain(..)
        .map(|row| {
            let ctx = EvalCtx::new(row.note, corpus, formulas);
            let vals = keys.iter().map(|(e, _)| ctx.eval(e)).collect();
            (vals, row)
        })
        .collect();
    // Decide once per key whether it orders as versions — detection reads the
    // whole column, so it cannot be done inside a pairwise comparison.
    let as_semver: Vec<bool> = view
        .sort
        .iter()
        .enumerate()
        .map(|(i, k)| sorts_as_semver(view, k.property(), decorated.iter().map(|d| &d.0[i])))
        .collect();
    decorated.sort_by(|a, b| {
        for (i, (_, desc)) in keys.iter().enumerate() {
            let ord = if as_semver[i] {
                semver_cmp(&a.0[i], &b.0[i])
            } else {
                a.0[i].loose_cmp(&b.0[i])
            };
            let ord = if *desc { ord.reverse() } else { ord };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
    *rows = decorated.into_iter().map(|(_, row)| row).collect();
}

/// Whether `prop` orders as versions: `sortAs` decides if it names the column,
/// otherwise the column auto-detects.
pub(crate) fn sorts_as_semver<'a>(
    view: &View,
    prop: &str,
    values: impl Iterator<Item = &'a Value>,
) -> bool {
    match view
        .interactive
        .as_ref()
        .and_then(|i| i.sort_as.get(prop))
        .map(|s| s.as_str())
    {
        Some("semver") => true,
        Some("text") => false,
        // Unknown value: fall back to detection rather than failing the build,
        // matching how the rest of the `.base` surface tolerates bad input.
        _ => semver::column_is_semver(values),
    }
}

/// Version ordering over raw values. Compares the same per-cell key the island
/// receives in the payload, so the static order and the client order are the
/// same order by construction (see `semver::column_sort_key`).
pub(crate) fn semver_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    semver::column_sort_key(a).cmp(&semver::column_sort_key(b))
}

/// Human-readable header for a column ref: `properties.<ref>.displayName` if set,
/// else the last segment humanized (`file.name` → `Name`, `note.due_date` →
/// `Due date`).
pub(crate) fn column_header(col: &str, base: &BaseFile) -> String {
    if let Some(cfg) = base.properties.get(col) {
        if let Some(dn) = &cfg.display_name {
            return dn.clone();
        }
    }
    let leaf = col.rsplit('.').next().unwrap_or(col);
    humanize(leaf)
}

fn humanize(s: &str) -> String {
    let spaced = s.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_table(
    rows: &[Row],
    view: &View,
    base: &BaseFile,
    columns: &[String],
    opts: &RenderOptions,
    corpus: &Corpus,
    formulas: &BTreeMap<String, Expr>,
    custom_summaries: &BTreeMap<String, Expr>,
) -> String {
    let group_by = view.group_by.as_ref();
    // `rows` is already truncated to the view's `limit` by render_view.
    let limit: Option<usize> = None;

    let mut html = String::new();
    // Reuse the same horizontal-scroll wrapper docgen uses for wide markdown tables.
    html.push_str("<div class=\"docgen-table-scroll\"><table class=\"docgen-base-table\">");

    // Header row.
    html.push_str("<thead><tr>");
    for col in columns {
        let width = view
            .column_size
            .get(col)
            .map(|w| format!(" style=\"width:{w}px\""))
            .unwrap_or_default();
        let data_col = if opts.interactive {
            format!(" data-col=\"{}\"", escape(col))
        } else {
            String::new()
        };
        html.push_str(&format!(
            "<th{width}{data_col}>{}</th>",
            escape(&column_header(col, base))
        ));
    }
    html.push_str("</tr></thead><tbody>");

    // Body, optionally grouped. A limit caps the total number of rows shown; once
    // it's reached we stop entirely, so a later group never emits an orphaned
    // header with no rows beneath it.
    let mut shown = 0usize;
    let grouped = group_rows(rows, group_by, corpus, formulas);
    'groups: for (group_label, group_rows) in &grouped {
        // Skip the whole group (header included) if the limit is already spent.
        if let Some(lim) = limit {
            if shown >= lim {
                break;
            }
        }
        if let Some(label) = group_label {
            let data_group = if opts.interactive {
                format!(" data-group=\"{}\"", escape(label))
            } else {
                String::new()
            };
            html.push_str(&format!(
                "<tr class=\"docgen-base-group\"{data_group}><td colspan=\"{}\">{}</td></tr>",
                columns.len().max(1),
                escape(label)
            ));
        }
        for row in group_rows {
            if let Some(lim) = limit {
                if shown >= lim {
                    break 'groups;
                }
            }
            if opts.interactive {
                html.push_str(&format!("<tr data-row=\"{}\">", row.id));
            } else {
                html.push_str("<tr>");
            }
            for col in columns {
                let val = row.cells.get(col).cloned().unwrap_or(Value::Null);
                html.push_str(&format!(
                    "<td>{}</td>",
                    render_cell(&val, col, row.note, opts, corpus)
                ));
            }
            html.push_str("</tr>");
            shown += 1;
        }
    }

    if rows.is_empty() {
        html.push_str(&format!(
            "<tr><td colspan=\"{}\" class=\"docgen-base-empty\">No results</td></tr>",
            columns.len().max(1)
        ));
    }
    html.push_str("</tbody>");

    // Summary footer row, if any column has a summary configured.
    if !view.summaries.is_empty() {
        html.push_str("<tfoot><tr>");
        for col in columns {
            let cell = match view.summaries.get(col) {
                Some(fname) => {
                    let values: Vec<Value> = rows
                        .iter()
                        .map(|r| r.cells.get(col).cloned().unwrap_or(Value::Null))
                        .collect();
                    escape(&crate::summary::summarize(
                        fname,
                        &values,
                        custom_summaries,
                        corpus,
                        formulas,
                    ))
                }
                None => String::new(),
            };
            html.push_str(&format!("<td>{cell}</td>"));
        }
        html.push_str("</tr></tfoot>");
    }

    html.push_str("</table></div>");
    html
}

/// Partition rows by the group-by property (rendered label). Returns groups in
/// group-label order (respecting the group direction); `None` label = ungrouped.
fn group_rows<'a, 'b>(
    rows: &'b [Row<'a>],
    group_by: Option<&GroupBy>,
    corpus: &Corpus,
    formulas: &BTreeMap<String, Expr>,
) -> Vec<(Option<String>, Vec<&'b Row<'a>>)> {
    let Some(gb) = group_by else {
        return vec![(None, rows.iter().collect())];
    };
    let expr = parse(gb.property()).unwrap_or(Expr::Null);
    // Group by rendered label (for the header text + dedup) but keep a
    // representative typed value per group so ordering is by the value's natural
    // order (numeric priorities 1 < 2 < 10), not lexicographic string order.
    let mut groups: Vec<(Value, String, Vec<&Row>)> = Vec::new();
    for row in rows {
        let val = EvalCtx::new(row.note, corpus, formulas).eval(&expr);
        let label = val.display();
        match groups.iter_mut().find(|(_, l, _)| *l == label) {
            Some((_, _, bucket)) => bucket.push(row),
            None => groups.push((val, label, vec![row])),
        }
    }
    groups.sort_by(|a, b| a.0.loose_cmp(&b.0));
    if gb.descending() {
        groups.reverse();
    }
    groups
        .into_iter()
        .map(|(_, label, rows)| (Some(label), rows))
        .collect()
}

/// Whether a column names the note's rendered body (`note.body` / `file.body`) —
/// a docgen extension that cards render as a full-width block.
fn is_body_col(col: &str) -> bool {
    col == "note.body" || col == "file.body"
}

fn render_cards(
    rows: &[Row],
    base: &BaseFile,
    columns: &[String],
    opts: &RenderOptions,
    corpus: &Corpus,
) -> String {
    // `rows` is already truncated to the view's `limit` by render_view.
    // A `note.body`/`file.body` column turns the cards into a single-column list,
    // each card carrying the note's rendered body beneath its fields — a readable
    // layout for long-form entries (e.g. release notes) rather than a tile grid.
    let has_body = columns.iter().any(|c| is_body_col(c));
    let container = if has_body {
        "docgen-base-cards docgen-base-cards--list"
    } else {
        "docgen-base-cards"
    };
    let mut html = format!("<div class=\"{container}\">");
    for row in rows.iter() {
        if opts.interactive {
            html.push_str(&format!(
                "<div class=\"docgen-base-card\" data-row=\"{}\">",
                row.id
            ));
        } else {
            html.push_str("<div class=\"docgen-base-card\">");
        }
        // Card title: the note's file name, linked to its page.
        html.push_str(&format!(
            "<div class=\"docgen-base-card__title\">{}</div>",
            render_cell(
                &Value::Str(row.note.basename.clone()),
                "file.name",
                row.note,
                opts,
                corpus
            )
        ));
        html.push_str("<dl class=\"docgen-base-card__fields\">");
        for col in columns {
            // The name columns become the card title; the body column becomes a
            // full-width block below — neither belongs in the field list.
            if col == "file.name" || col == "file.basename" || is_body_col(col) {
                continue;
            }
            let val = row.cells.get(col).cloned().unwrap_or(Value::Null);
            if val.is_empty() {
                continue;
            }
            html.push_str(&format!(
                "<dt>{}</dt><dd>{}</dd>",
                escape(&column_header(col, base)),
                render_cell(&val, col, row.note, opts, corpus)
            ));
        }
        html.push_str("</dl>");
        // Body: the note's already-rendered HTML, emitted verbatim (not escaped).
        if has_body && !row.note.body.is_empty() {
            html.push_str(&format!(
                "<div class=\"docgen-base-card__body\">{}</div>",
                row.note.body
            ));
        }
        html.push_str("</div>");
    }
    if rows.is_empty() {
        html.push_str("<div class=\"docgen-base-empty\">No results</div>");
    }
    html.push_str("</div>");
    html
}

fn render_list(
    rows: &[Row],
    columns: &[String],
    opts: &RenderOptions,
    corpus: &Corpus,
    formulas: &BTreeMap<String, Expr>,
) -> String {
    let _ = (columns, formulas);
    let mut html = String::from("<ul class=\"docgen-base-list\">");
    for row in rows {
        let li_open = if opts.interactive {
            format!("<li data-row=\"{}\">", row.id)
        } else {
            "<li>".to_string()
        };
        html.push_str(&format!(
            "{li_open}{}</li>",
            render_cell(
                &Value::Str(row.note.basename.clone()),
                "file.name",
                row.note,
                opts,
                corpus
            )
        ));
    }
    if rows.is_empty() {
        html.push_str("<li class=\"docgen-base-empty\">No results</li>");
    }
    html.push_str("</ul>");
    html
}

/// Render a single cell value to HTML, hyperlinking note references. The
/// `file.name`/`file.basename`/`file.path`/`file.file` columns link to the row
/// note's own page; `Link` values resolve to their target note's page.
fn render_cell(
    val: &Value,
    col: &str,
    note: &Note,
    opts: &RenderOptions,
    corpus: &Corpus,
) -> String {
    // Self-referential file columns → a link to this note's page. Obsidian shows
    // the note's title (basename, no extension) as the link text for the name/
    // basename/file columns, and the full path for the path column — regardless of
    // the raw metadata value (which keeps its extension for filters).
    if matches!(col, "file.name" | "file.basename" | "file.file") && !note.slug.is_empty() {
        return link_html(&opts.note_url(&note.slug), &note.basename);
    }
    if col == "file.path" && !note.slug.is_empty() {
        return link_html(&opts.note_url(&note.slug), &note.path);
    }
    render_value(val, opts, corpus)
}

/// Render a value to HTML (recursively for lists), turning links into anchors.
fn render_value(val: &Value, opts: &RenderOptions, corpus: &Corpus) -> String {
    match val {
        Value::Link(l) => {
            let text = l
                .display
                .clone()
                .unwrap_or_else(|| l.basename().to_string());
            match resolve_link_slug(l, corpus) {
                Some(slug) => link_html(&opts.note_url(&slug), &text),
                None => format!(
                    "<span class=\"docgen-base-link--unresolved\">{}</span>",
                    escape(&text)
                ),
            }
        }
        Value::List(items) => items
            .iter()
            .map(|v| render_value(v, opts, corpus))
            .collect::<Vec<_>>()
            .join("<span class=\"docgen-base-sep\">, </span>"),
        Value::Bool(b) => {
            // Render booleans as a checkbox glyph, matching Obsidian's checkmark cells.
            if *b {
                "<span class=\"docgen-base-check\">✓</span>".to_string()
            } else {
                "<span class=\"docgen-base-check docgen-base-check--off\">✗</span>".to_string()
            }
        }
        Value::Null => String::new(),
        other => escape(&other.display()),
    }
}

/// Resolve a link to a corpus note's slug (by basename, case-insensitive).
fn resolve_link_slug(link: &crate::value::BaseLink, corpus: &Corpus) -> Option<String> {
    let want = link.basename().to_lowercase();
    corpus
        .notes
        .iter()
        .find(|n| {
            n.basename.to_lowercase() == want
                || n.path.to_lowercase() == format!("{}.md", link.path.to_lowercase())
        })
        .map(|n| n.slug.clone())
}

fn link_html(url: &str, text: &str) -> String {
    format!("<a href=\"{}\">{}</a>", escape(url), escape(text))
}

/// A styled, inert error block for a malformed base (parse failure). Detailed
/// message, never a panic — the analogue of the PlantUML error component.
pub fn error_block(message: &str) -> String {
    format!(
        "<div class=\"docgen-base-error\"><strong>Base error:</strong> {}</div>",
        escape(message)
    )
}

/// Minimal HTML-escaping (the crate is standalone; no docgen-core dependency).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_base;
    use crate::value::{BaseDate, BaseLink, Value};

    fn date(y: i64, mo: u32, d: u32) -> Value {
        Value::Date(BaseDate {
            year: y,
            month: mo,
            day: d,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            has_time: false,
        })
    }

    /// Extract the JSON text inside the `docgen-base-data` script element.
    fn extract_payload(html: &str) -> String {
        let marker = "class=\"docgen-base-data\">";
        let start = html.find(marker).expect("payload script present") + marker.len();
        let rest = &html[start..];
        let end = rest.find("</script>").expect("payload script closes");
        rest[..end].to_string()
    }

    fn note(slug: &str, name: &str, props: &[(&str, Value)]) -> Note {
        let mut n = Note::default();
        n.slug = slug.to_string();
        n.basename = name.to_string();
        n.name = format!("{name}.md");
        n.path = format!("{name}.md");
        n.ext = "md".into();
        for (k, v) in props {
            n.properties.insert(k.to_string(), v.clone());
        }
        n
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            base: String::new(),
            default_view_name: "Base".into(),
            interactive: false,
            block_index: 0,
        }
    }

    fn interactive_opts() -> RenderOptions {
        RenderOptions {
            base: String::new(),
            default_view_name: "Base".into(),
            interactive: true,
            block_index: 0,
        }
    }

    #[test]
    fn renders_table_with_filter_and_columns() {
        let base = parse_base(
            "filters:\n  and:\n    - file.hasTag(\"book\")\nviews:\n  - type: table\n    name: Books\n    order:\n      - file.name\n      - note.rating\n",
        )
        .unwrap();
        let mut a = note("a", "Dune", &[("rating", Value::Number(5.0))]);
        a.tags = vec!["book".into()];
        let mut b = note("b", "NotABook", &[("rating", Value::Number(1.0))]);
        b.tags = vec!["film".into()];
        let corpus = Corpus::new(vec![a, b]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("docgen-base-table"));
        assert!(html.contains("Books")); // view title
        assert!(html.contains(">Dune<")); // linked file name
        assert!(html.contains("href=\"/a\"")); // note URL
        assert!(!html.contains("NotABook")); // filtered out
        assert!(html.contains("<th>Rating</th>")); // humanized header
    }

    #[test]
    fn sort_descending_and_limit() {
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    sort:\n      - property: file.name\n        direction: DESC\n    limit: 2\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "Apple", &[]),
            note("b", "Banana", &[]),
            note("c", "Cherry", &[]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        // Descending: Cherry, Banana appear; Apple cut by the limit of 2.
        let cherry = html.find("Cherry").unwrap();
        let banana = html.find("Banana").unwrap();
        assert!(cherry < banana);
        assert!(!html.contains("Apple"));
    }

    #[test]
    fn resolves_wikilink_cells_to_pages() {
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name, note.author]\n").unwrap();
        let corpus = Corpus::new(vec![
            note(
                "books/dune",
                "Dune",
                &[("author", Value::Link(BaseLink::new("Herbert")))],
            ),
            note("people/herbert", "Herbert", &[]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        // The author link resolves to the Herbert note's page.
        assert!(html.contains("href=\"/people/herbert\""));
    }

    #[test]
    fn group_by_renders_group_rows() {
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    groupBy: note.status\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("done".into()))]),
            note("b", "B", &[("status", Value::Str("todo".into()))]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("docgen-base-group"));
        assert!(html.contains(">done<"));
        assert!(html.contains(">todo<"));
    }

    #[test]
    fn summary_footer() {
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.price]\n    summaries:\n      note.price: Sum\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("price", Value::Number(2.0))]),
            note("b", "B", &[("price", Value::Number(3.0))]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("<tfoot>"));
        assert!(html.contains(">5<")); // sum of prices
    }

    #[test]
    fn empty_results_message() {
        let base =
            parse_base("filters:\n  and:\n    - file.hasTag(\"nope\")\nviews:\n  - type: table\n")
                .unwrap();
        let corpus = Corpus::new(vec![note("a", "A", &[])]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("No results"));
    }

    /// Row order for a `sort:` on `note.version`, top to bottom.
    fn version_order(extra_view_yaml: &str, versions: &[&str]) -> Vec<String> {
        let base = parse_base(&format!(
            "views:\n  - type: table\n    order: [file.name, note.version]\n    sort:\n      - property: note.version\n        direction: ASC\n{extra_view_yaml}"
        ))
        .unwrap();
        let corpus = Corpus::new(
            versions
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let name = format!("n{i}");
                    note(
                        Box::leak(name.clone().into_boxed_str()),
                        Box::leak(name.into_boxed_str()),
                        &[("version", Value::Str((*v).into()))],
                    )
                })
                .collect(),
        );
        let html = render_base(&base, &corpus, &opts());
        // Recover order from the rendered cells rather than re-sorting here, so
        // the test observes what a reader actually sees.
        versions
            .iter()
            .map(|v| (html.find(&format!(">{v}<")).unwrap_or(usize::MAX), *v))
            .fold(Vec::new(), |mut acc, (pos, v)| {
                acc.push((pos, v.to_string()));
                acc
            })
            .into_iter()
            .filter(|(p, _)| *p != usize::MAX)
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_values()
            .collect()
    }

    #[test]
    fn version_column_sorts_as_semver_not_text() {
        // Lexically this would be 1.0.23, 1.19.20, 1.2.12.
        assert_eq!(
            version_order("", &["1.2.12", "1.19.20", "1.0.23"]),
            ["1.0.23", "1.2.12", "1.19.20"]
        );
    }

    #[test]
    fn sort_as_text_opts_out_of_semver() {
        assert_eq!(
            version_order(
                "    docgenInteractive:\n      sortAs:\n        note.version: text\n",
                &["1.2.12", "1.19.20", "1.0.23"]
            ),
            ["1.0.23", "1.19.20", "1.2.12"]
        );
    }

    #[test]
    fn mixed_column_falls_back_to_text() {
        // One non-version disqualifies the column, so ordering stays lexical
        // ("1.19.20" before "1.2.12") rather than half-numeric.
        assert_eq!(
            version_order("", &["1.2.12", "nightly", "1.19.20"]),
            ["1.19.20", "1.2.12", "nightly"]
        );
    }

    #[test]
    fn sort_as_semver_forces_detection_and_sorts_junk_last() {
        assert_eq!(
            version_order(
                "    docgenInteractive:\n      sortAs:\n        note.version: semver\n",
                &["1.2.12", "nightly", "1.19.20"]
            ),
            ["1.2.12", "1.19.20", "nightly"]
        );
    }

    #[test]
    fn cards_view() {
        let base =
            parse_base("views:\n  - type: cards\n    order: [file.name, note.rating]\n").unwrap();
        let corpus = Corpus::new(vec![note("a", "A", &[("rating", Value::Number(5.0))])]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("docgen-base-cards"));
        assert!(html.contains("docgen-base-card__title"));
    }

    #[test]
    fn grouped_limit_emits_no_orphan_group_header() {
        // limit 2, groups done(A,B) and todo(C,D): the 'todo' header must NOT appear
        // because the limit is consumed by the 'done' group.
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    groupBy: note.status\n    limit: 2\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("done".into()))]),
            note("b", "B", &[("status", Value::Str("done".into()))]),
            note("c", "C", &[("status", Value::Str("todo".into()))]),
            note("d", "D", &[("status", Value::Str("todo".into()))]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains(">done<"));
        assert!(
            !html.contains(">todo<"),
            "orphan group header must not appear"
        );
    }

    #[test]
    fn group_by_orders_numerically_not_lexically() {
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    groupBy: note.priority\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("priority", Value::Number(2.0))]),
            note("b", "B", &[("priority", Value::Number(10.0))]),
            note("c", "C", &[("priority", Value::Number(1.0))]),
        ]);
        let html = render_base(&base, &corpus, &opts());
        // Groups appear in numeric order 1, 2, 10 — not lexical "1","10","2".
        let p1 = html.find(">1<").unwrap();
        let p2 = html.find(">2<").unwrap();
        let p10 = html.find(">10<").unwrap();
        assert!(p1 < p2 && p2 < p10, "expected 1 < 2 < 10 group order");
    }

    #[test]
    fn cards_view_renders_note_body_as_block_and_single_column() {
        let base = parse_base(
            "views:\n  - type: cards\n    order: [file.name, note.kind, note.body]\n",
        )
        .unwrap();
        let mut n = note("v0-8-1", "v0-8-1", &[("kind", Value::Str("patch".into()))]);
        n.body = "<p>The <strong>S3</strong> TLS fix.</p>".into();
        let corpus = Corpus::new(vec![n]);
        let html = render_base(&base, &corpus, &opts());
        // Single-column list layout, plus the body emitted verbatim in its block.
        assert!(html.contains("docgen-base-cards--list"));
        assert!(html.contains(
            "<div class=\"docgen-base-card__body\"><p>The <strong>S3</strong> TLS fix.</p></div>"
        ));
        // The body column is NOT also emitted as a <dt>/<dd> field pair.
        assert!(!html.contains("<dt>Body</dt>"));
        // A normal field still renders.
        assert!(html.contains("patch"));
    }

    #[test]
    fn cards_view_without_body_stays_a_grid() {
        let base =
            parse_base("views:\n  - type: cards\n    order: [file.name, note.kind]\n").unwrap();
        let corpus = Corpus::new(vec![note("a", "A", &[("kind", Value::Str("x".into()))])]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("docgen-base-cards"));
        assert!(!html.contains("docgen-base-cards--list"));
    }

    #[test]
    fn cards_view_honors_display_name() {
        let base = parse_base(
            "properties:\n  note.status:\n    displayName: Current Status\nviews:\n  - type: cards\n    order: [file.name, note.status]\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![note(
            "a",
            "A",
            &[("status", Value::Str("open".into()))],
        )]);
        let html = render_base(&base, &corpus, &opts());
        assert!(html.contains("Current Status"));
        assert!(!html.contains("<dt>Status</dt>"));
    }

    #[test]
    fn malformed_yaml_yields_error_block() {
        let html = render_base_source("filters: [unclosed\n", &Corpus::default(), &opts());
        assert!(html.contains("docgen-base-error"));
        assert!(html.contains("could not parse"));
    }

    #[test]
    fn base_url_prefix_applied() {
        let base = parse_base("views:\n  - type: table\n    order: [file.name]\n").unwrap();
        let corpus = Corpus::new(vec![note("guide/a", "A", &[])]);
        let o = RenderOptions {
            base: "/docs".into(),
            default_view_name: "Base".into(),
            interactive: false,
            block_index: 0,
        };
        let html = render_base(&base, &corpus, &o);
        assert!(html.contains("href=\"/docs/guide/a\""));
    }

    // ---- Interactive mode (M1) ----

    #[test]
    fn interactive_false_is_pure_static() {
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name, note.rating]\n").unwrap();
        let corpus = Corpus::new(vec![note("a", "A", &[("rating", Value::Number(5.0))])]);
        let html = render_base(&base, &corpus, &opts());
        assert!(!html.contains("data-"));
        assert!(!html.contains("docgen-base-data"));
    }

    #[test]
    fn interactive_true_adds_hooks() {
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name, note.rating]\n").unwrap();
        let corpus = Corpus::new(vec![note("a", "A", &[("rating", Value::Number(5.0))])]);
        let html = render_base(&base, &corpus, &interactive_opts());
        // `{block}-{view}`: block 0 (a standalone base), first view.
        assert!(html.contains("data-base-view=\"0-0\""));
        assert!(html.contains("<script type=\"application/json\" class=\"docgen-base-data\">"));
        assert!(html.contains("<tr data-row=\"0\""));
        assert!(html.contains("<th data-col=\"file.name\""));
    }

    #[test]
    fn payload_parses_and_limit_is_applied() {
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name]\n    limit: 1\n").unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[]),
            note("b", "B", &[]),
            note("c", "C", &[]),
        ]);
        let html = render_base(&base, &corpus, &interactive_opts());
        // `limit` is a row cap, not a page size: it truncates in interactive mode
        // exactly as it does statically, so the rows it cut never reach the client.
        assert_eq!(html.matches("data-row=").count(), 1);
        let payload = extract_payload(&html);
        let v: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        assert_eq!(v["v"], 1);
        assert_eq!(v["columns"].as_array().unwrap().len(), 1);
        // The payload must never carry a row the DOM lacks — the island would hold
        // an id with no node to move or hide.
        assert_eq!(v["rows"].as_array().unwrap().len(), 1);
        // `limit` is not shipped: already applied, and re-applying would double-cap.
        assert_eq!(v["view"]["limit"], serde_json::Value::Null);
        // A row cap is not pagination. 1 row => no pager.
        assert_eq!(v["controls"]["pageSize"], 0);
    }

    /// A key docgen does not know in ITS OWN namespace is always an author error
    /// (Obsidian never reads this block), so it must not vanish silently. The view
    /// still renders — only the mistyped knob is lost.
    #[test]
    fn unknown_docgen_interactive_key_warns_but_still_renders() {
        let corpus = Corpus::new(vec![note("a", "A", &[])]);
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    docgenInteractive:\n      pagesize: 25\n",
        )
        .unwrap();
        let html = render_base(&base, &corpus, &interactive_opts());
        assert!(
            html.contains("docgen-base-warning"),
            "warning must be shown"
        );
        assert!(html.contains("pagesize"), "must name the offending key");
        assert!(
            html.contains("did you mean `pageSize`?"),
            "a case slip is the likely typo; name the intended key"
        );
        // The view still renders: degradation, not failure.
        assert!(html.contains("docgen-base-table"));
        assert!(html.contains(">A<"));
        // ...and the typo really did NOT take effect (default paging, not 25).
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&html)).unwrap();
        assert_eq!(v["controls"]["pageSize"], 0);
    }

    /// The warning must not fire on a correct block, or it is noise.
    #[test]
    fn known_docgen_interactive_keys_do_not_warn() {
        let corpus = Corpus::new(vec![note("a", "A", &[])]);
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    docgenInteractive:\n      pageSize: 25\n      search: false\n      maxEnum: 10\n",
        )
        .unwrap();
        let html = render_base(&base, &corpus, &interactive_opts());
        assert!(!html.contains("docgen-base-warning"));
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&html)).unwrap();
        assert_eq!(v["controls"]["pageSize"], 25);
        assert_eq!(v["controls"]["search"], false);
    }

    /// `limit` must cap every view type. `render_list` never applied it at all —
    /// not even in static mode — because it was the one renderer that did not
    /// receive `view`. Capping centrally in `render_view` covers all three.
    #[test]
    fn limit_truncates_every_view_type() {
        let corpus = Corpus::new(vec![
            note("a", "A", &[]),
            note("b", "B", &[]),
            note("c", "C", &[]),
        ]);
        for (ty, marker) in [
            ("table", "<tr data-row="),
            ("cards", "docgen-base-card\" data-row="),
            ("list", "<li data-row="),
        ] {
            let base = parse_base(&format!(
                "views:\n  - type: {ty}\n    order: [file.name]\n    limit: 2\n"
            ))
            .unwrap();
            let html = render_base(&base, &corpus, &interactive_opts());
            assert_eq!(
                html.matches(marker).count(),
                2,
                "{ty} view must honor limit: 2"
            );
            assert!(!html.contains(">C<"), "{ty}: the cut row must be absent");
        }
    }

    #[test]
    fn cell_projection_shapes() {
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.due, note.tags, note.n, note.e]\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![note(
            "a",
            "A",
            &[
                ("due", date(2026, 7, 15)),
                (
                    "tags",
                    Value::List(vec![Value::Str("api".into()), Value::Str("db".into())]),
                ),
                ("n", Value::Number(42.0)),
                ("e", Value::Null),
            ],
        )]);
        let html = render_base(&base, &corpus, &interactive_opts());
        let payload = extract_payload(&html);
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let cells = &v["rows"][0]["cells"];
        // Date cell → epoch present.
        assert_eq!(cells["note.due"]["t"], "date");
        assert!(cells["note.due"]["epoch"].is_number());
        // List cell → f tokens.
        assert_eq!(cells["note.tags"]["t"], "list");
        assert_eq!(cells["note.tags"]["f"], serde_json::json!(["api", "db"]));
        assert_eq!(cells["note.tags"]["d"], "api, db");
        // Numeric cell → num present.
        assert_eq!(cells["note.n"]["t"], "num");
        assert_eq!(cells["note.n"]["num"], 42.0);
        // Null/empty cell → t null + empty true.
        assert_eq!(cells["note.e"]["t"], "null");
        assert_eq!(cells["note.e"]["empty"], true);
    }

    #[test]
    fn type_inference_dates_enum_text() {
        // All-dates column → type date, filter date.
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name, note.due]\n").unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("due", date(2026, 1, 1))]),
            note("b", "B", &[("due", date(2026, 2, 2))]),
        ]);
        let html = render_base(&base, &corpus, &interactive_opts());
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&html)).unwrap();
        let due = v["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.due")
            .unwrap();
        assert_eq!(due["type"], "date");
        assert_eq!(due["filter"], "date");

        // Low-cardinality string → enum.
        let corpus2 = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("open".into()))]),
            note("b", "B", &[("status", Value::Str("open".into()))]),
            note("c", "C", &[("status", Value::Str("done".into()))]),
        ]);
        let base2 =
            parse_base("views:\n  - type: table\n    order: [file.name, note.status]\n").unwrap();
        let html2 = render_base(&base2, &corpus2, &interactive_opts());
        let v2: serde_json::Value = serde_json::from_str(&extract_payload(&html2)).unwrap();
        let st = v2["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.status")
            .unwrap();
        assert_eq!(st["type"], "str");
        assert_eq!(st["filter"], "enum");

        // High-cardinality (> maxEnum) string → text (maxEnum lowered to 2).
        let base3 = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.status]\n    docgenInteractive:\n      maxEnum: 2\n",
        )
        .unwrap();
        let corpus3 = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("one".into()))]),
            note("b", "B", &[("status", Value::Str("two".into()))]),
            note("c", "C", &[("status", Value::Str("three".into()))]),
        ]);
        let html3 = render_base(&base3, &corpus3, &interactive_opts());
        let v3: serde_json::Value = serde_json::from_str(&extract_payload(&html3)).unwrap();
        let st3 = v3["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.status")
            .unwrap();
        assert_eq!(st3["filter"], "text");
    }

    #[test]
    fn override_precedence_and_enabled_helper() {
        // filters override forces text even though auto would pick enum.
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.status]\n    docgenInteractive:\n      filters:\n        note.status: text\n",
        )
        .unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("open".into()))]),
            note("b", "B", &[("status", Value::Str("open".into()))]),
        ]);
        let html = render_base(&base, &corpus, &interactive_opts());
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&html)).unwrap();
        let st = v["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.status")
            .unwrap();
        assert_eq!(st["filter"], "text");

        // enabled:false → view_interactive_enabled reports false.
        let disabled =
            parse_base("views:\n  - type: table\n    docgenInteractive:\n      enabled: false\n")
                .unwrap();
        assert!(!crate::view_interactive_enabled(
            &disabled,
            &disabled.views[0]
        ));
        // A plain view is enabled.
        let plain = parse_base("views:\n  - type: table\n").unwrap();
        assert!(crate::view_interactive_enabled(&plain, &plain.views[0]));
        // Base-level docgenInteractive:false disables all views.
        let base_off = parse_base("docgenInteractive: false\nviews:\n  - type: table\n").unwrap();
        assert!(!crate::view_interactive_enabled(
            &base_off,
            &base_off.views[0]
        ));
    }

    #[test]
    fn docgen_interactive_false_opts_out_end_to_end() {
        // Same base + corpus, rendered with interactive:true opts. The disabled
        // base must emit NO interactive hooks/payload (pure static); the plain
        // base must emit them — proving the renderer honors the opt-out.
        let corpus = Corpus::new(vec![
            note("a", "A", &[("status", Value::Str("open".into()))]),
            note("b", "B", &[("status", Value::Str("done".into()))]),
        ]);

        let disabled =
            parse_base("docgenInteractive: false\nviews:\n  - type: table\n    order: [file.name, note.status]\n")
                .unwrap();
        let off = render_base(&disabled, &corpus, &interactive_opts());
        assert!(!off.contains("data-base-view"));
        assert!(!off.contains("docgen-base-data"));
        // Still rendered the static table content.
        assert!(off.contains("docgen-base-table"));

        let plain =
            parse_base("views:\n  - type: table\n    order: [file.name, note.status]\n").unwrap();
        let on = render_base(&plain, &corpus, &interactive_opts());
        assert!(on.contains("data-base-view"));
        assert!(on.contains("docgen-base-data"));
    }

    #[test]
    fn list_column_facet_cardinality_capped() {
        fn list(items: &[&str]) -> Value {
            Value::List(items.iter().map(|s| Value::Str((*s).into())).collect())
        }
        // A low-cardinality list column → enum.
        let base =
            parse_base("views:\n  - type: table\n    order: [file.name, note.tags]\n").unwrap();
        let corpus = Corpus::new(vec![
            note("a", "A", &[("tags", list(&["x", "y"]))]),
            note("b", "B", &[("tags", list(&["y"]))]),
        ]);
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
            &base,
            &corpus,
            &interactive_opts(),
        )))
        .unwrap();
        let col = v["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.tags")
            .unwrap();
        assert_eq!(col["type"], "list");
        assert_eq!(col["filter"], "enum");

        // A high-cardinality list column (distinct tokens > maxEnum) → text.
        let base2 = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.tags]\n    docgenInteractive:\n      maxEnum: 2\n",
        )
        .unwrap();
        let corpus2 = Corpus::new(vec![
            note("a", "A", &[("tags", list(&["a", "b"]))]),
            note("b", "B", &[("tags", list(&["c", "d"]))]),
        ]);
        let v2: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
            &base2,
            &corpus2,
            &interactive_opts(),
        )))
        .unwrap();
        let col2 = v2["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["key"] == "note.tags")
            .unwrap();
        assert_eq!(col2["type"], "list");
        assert_eq!(
            col2["filter"], "text",
            "4 distinct tokens > maxEnum 2 → text"
        );
    }

    #[test]
    fn payload_escapes_all_angle_brackets() {
        // Every `<` must be escaped — not just `</` — so no `</script`, `<!--`, or
        // `<script` sequence can steer the HTML tokenizer out of the data script
        // (a `<!--<script>` value contains no `</` yet still breaks naive escaping).
        let base = parse_base("views:\n  - type: table\n    order: [file.name, note.x]\n").unwrap();
        let corpus = Corpus::new(vec![note(
            "a",
            "A",
            &[("x", Value::Str("</script><!--<script><b>hi".into()))],
        )]);
        let html = render_base(&base, &corpus, &interactive_opts());
        let region = extract_payload(&html);
        // No literal `<` at all leaks into the embedded JSON.
        assert!(
            !region.contains('<'),
            "payload must contain no raw '<': {region}"
        );
        assert!(region.contains("\\u003c"), "escaped form present");
        // And the payload still parses back to the exact original string.
        let v: serde_json::Value = serde_json::from_str(&region).unwrap();
        assert_eq!(
            v["rows"][0]["cells"]["note.x"]["d"],
            "</script><!--<script><b>hi"
        );
    }

    /// `groupBy` is table-only (see the graceful-degradation contract in
    /// website/docs/features/bases.md): cards/list parse it and render ungrouped.
    /// The payload has to say so too — the island derives `V.grouped` from this
    /// field, and a truthy `grouped` suppresses the sort dropdown that IS the only
    /// sort affordance a cards/list view has. Emitting it for a view that renders
    /// ungrouped left the reader with no way to sort at all.
    #[test]
    fn group_by_is_not_emitted_for_non_table_views() {
        let corpus = Corpus::new(vec![note("a", "A", &[("status", Value::Str("x".into()))])]);
        for ty in ["cards", "list"] {
            let base = parse_base(&format!(
                "views:\n  - type: {ty}\n    order: [file.name, note.status]\n    groupBy: note.status\n"
            ))
            .unwrap();
            let v: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
                &base,
                &corpus,
                &interactive_opts(),
            )))
            .unwrap();
            assert_eq!(
                v["view"]["groupBy"],
                serde_json::Value::Null,
                "{ty} renders ungrouped, so its payload must not claim a groupBy"
            );
        }

        // The table case is the control: it really does group, so it keeps the field.
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.status]\n    groupBy: note.status\n",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
            &base,
            &corpus,
            &interactive_opts(),
        )))
        .unwrap();
        assert_eq!(v["view"]["groupBy"]["col"], "note.status");
    }

    /// The island skips its corrective reorder when the requested sort already
    /// matches the order the SSR DOM is in. It therefore needs to know that real
    /// order — which is `view.sort`, since `apply_sort` never consults
    /// `defaultSort`. Reporting `controls.sort` (= defaultSort) as the DOM order
    /// made the island think the rows were already arranged that way.
    #[test]
    fn payload_reports_the_ssr_sort_not_the_default_sort_override() {
        let corpus = Corpus::new(vec![note("a", "A", &[("date", Value::Number(1.0))])]);
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name, note.date]\n    sort:\n      - property: note.date\n        direction: DESC\n    docgenInteractive:\n      defaultSort:\n        - property: file.name\n          direction: ASC\n",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
            &base,
            &corpus,
            &interactive_opts(),
        )))
        .unwrap();

        // What the rows are actually arranged by on the server.
        assert_eq!(v["view"]["sort"][0]["col"], "note.date");
        assert_eq!(v["view"]["sort"][0]["desc"], true);
        // What the controls should open on — deliberately different.
        assert_eq!(v["controls"]["sort"][0]["col"], "file.name");
        assert_eq!(v["controls"]["sort"][0]["desc"], false);
    }

    /// With no `sort:`, `apply_sort` early-returns and the rows keep corpus order.
    /// An empty `view.sort` is what tells the island that, so a `defaultSort` will
    /// correctly be seen as a change and applied on first render.
    #[test]
    fn payload_ssr_sort_is_empty_when_the_view_does_not_sort() {
        let corpus = Corpus::new(vec![note("a", "A", &[])]);
        let base = parse_base(
            "views:\n  - type: table\n    order: [file.name]\n    docgenInteractive:\n      defaultSort:\n        - property: file.name\n          direction: ASC\n",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&extract_payload(&render_base(
            &base,
            &corpus,
            &interactive_opts(),
        )))
        .unwrap();
        assert_eq!(v["view"]["sort"].as_array().unwrap().len(), 0);
        assert_eq!(v["controls"]["sort"][0]["col"], "file.name");
    }

    /// `data-base-view` has to be unique per PAGE, not per base: the island uses it
    /// to namespace URL-hash segments and facet-panel DOM ids. Two embedded blocks
    /// both starting their enumeration at 0 made each block strip the other's URL
    /// state and emit colliding element ids.
    #[test]
    fn view_ids_are_namespaced_by_block() {
        let corpus = Corpus::new(vec![note("a", "A", &[])]);
        let base = parse_base("views:\n  - type: table\n    order: [file.name]\n").unwrap();

        let first = render_base(&base, &corpus, &interactive_opts());
        let second = render_base(
            &base,
            &corpus,
            &RenderOptions {
                block_index: 1,
                ..interactive_opts()
            },
        );
        assert!(first.contains("data-base-view=\"0-0\""));
        assert!(second.contains("data-base-view=\"1-0\""));
    }
}
