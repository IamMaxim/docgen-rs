//! Static HTML rendering of a base's views. Produces a self-contained
//! `<div class="docgen-base">` — no scripts, no runtime — styled by the shared
//! `docgen.css`. Each view (table/cards/list) is filtered, sorted, grouped, and
//! limited exactly as configured, then rendered to server-side HTML.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::Expr;
use crate::eval::EvalCtx;
use crate::filter;
use crate::model::{BaseFile, GroupBy, View};
use crate::note::{Corpus, Note};
use crate::parser::parse;
use crate::value::Value;

/// Rendering configuration supplied by the host (docgen).
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Site base path (e.g. `/docs` or empty) prefixed onto internal note URLs.
    pub base: String,
    /// Fallback title used for a view with no `name`.
    pub default_view_name: String,
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
            base,
            corpus,
            opts,
            &formulas,
            &custom_summaries,
        ));
    }
    for view in &base.views {
        out.push_str(&render_view(
            view,
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
struct Row<'a> {
    note: &'a Note,
    cells: BTreeMap<String, Value>,
}

fn render_view(
    view: &View,
    base: &BaseFile,
    corpus: &Corpus,
    opts: &RenderOptions,
    formulas: &BTreeMap<String, Expr>,
    custom_summaries: &BTreeMap<String, Expr>,
) -> String {
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
            Row { note: n, cells }
        })
        .collect();

    // Sort by the view's sort keys (evaluated on the fly), stable, in order.
    apply_sort(&mut rows, view, corpus, formulas);

    // Group (optional), then limit within the whole view.
    let name = view
        .name
        .clone()
        .unwrap_or_else(|| opts.default_view_name.clone());

    let body = match view.view_type.as_str() {
        "cards" => render_cards(&rows, view, base, &columns, opts, corpus),
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
    section.push_str("<section class=\"docgen-base-view\">");
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
    decorated.sort_by(|a, b| {
        for (i, (_, desc)) in keys.iter().enumerate() {
            let ord = a.0[i].loose_cmp(&b.0[i]);
            let ord = if *desc { ord.reverse() } else { ord };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
    *rows = decorated.into_iter().map(|(_, row)| row).collect();
}

/// Human-readable header for a column ref: `properties.<ref>.displayName` if set,
/// else the last segment humanized (`file.name` → `Name`, `note.due_date` →
/// `Due date`).
fn column_header(col: &str, base: &BaseFile) -> String {
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
    let limit = view.limit;

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
        html.push_str(&format!(
            "<th{width}>{}</th>",
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
            html.push_str(&format!(
                "<tr class=\"docgen-base-group\"><td colspan=\"{}\">{}</td></tr>",
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
            html.push_str("<tr>");
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

fn render_cards(
    rows: &[Row],
    view: &View,
    base: &BaseFile,
    columns: &[String],
    opts: &RenderOptions,
    corpus: &Corpus,
) -> String {
    let limit = view.limit.unwrap_or(usize::MAX);
    let mut html = String::from("<div class=\"docgen-base-cards\">");
    for row in rows.iter().take(limit) {
        html.push_str("<div class=\"docgen-base-card\">");
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
            if col == "file.name" || col == "file.basename" {
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
        html.push_str("</dl></div>");
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
        html.push_str(&format!(
            "<li>{}</li>",
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
    use crate::value::{BaseLink, Value};

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
        };
        let html = render_base(&base, &corpus, &o);
        assert!(html.contains("href=\"/docs/guide/a\""));
    }
}
