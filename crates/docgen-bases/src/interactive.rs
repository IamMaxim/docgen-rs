//! Interactive-bases payload builder (M1 of the "interactive bases" feature).
//!
//! Pure, no I/O. Given the same filtered/sorted rows the static renderer emits,
//! this produces the compact JSON payload (contract v1) that the client-side
//! island (`islands/bases.js`, M4) hydrates against. The payload carries only the
//! *keys* needed for sort/filter/search/facet — never display HTML — so rendering
//! stays single-source in `render.rs`.
//!
//! See `.overnight/interactive-bases/SCHEMA.md` for the frozen contract.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::{json, Map, Value as J};

use crate::model::{BaseFile, View, ViewInteractive};
use crate::render::{self, column_header, RenderOptions};
use crate::semver;
use crate::value::Value;

/// Default enum-vs-text cardinality threshold.
const DEFAULT_MAX_ENUM: usize = 40;

/// A row as seen by the payload builder: a stable id plus its evaluated cells.
pub(crate) struct RowView<'a> {
    pub id: usize,
    pub cells: &'a BTreeMap<String, Value>,
}

/// Whether the interactive island should be enabled for `view` within `base`.
///
/// The actual gating (whether to emit interactive HTML at all) is performed by
/// the host in M3; this helper encodes the precedence so both sides agree:
/// base-level `docgenInteractive: false` disables everything; a per-view
/// `docgenInteractive.enabled: false` disables that view; otherwise enabled.
pub fn view_interactive_enabled(base: &BaseFile, view: &View) -> bool {
    if let Some(toggle) = &base.docgen_interactive {
        if !toggle.enabled() {
            return false;
        }
    }
    if let Some(iv) = &view.interactive {
        if iv.enabled == Some(false) {
            return false;
        }
    }
    true
}

/// The concrete type tag for a value, per SCHEMA "Cell object".
fn type_tag(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "num",
        Value::Str(_) => "str",
        Value::Date(_) => "date",
        Value::Duration(_) => "dur",
        Value::List(_) => "list",
        Value::Object(_) => "obj",
        Value::Link(_) => "link",
    }
}

/// Project one cell to its compact JSON object (omitting inapplicable fields).
///
/// `semver` marks a version column: every cell in one carries an `sv` sort key
/// so the island orders it by string comparison instead of parsing versions
/// itself (see `semver::column_sort_key`).
fn project_cell(v: &Value, semver_col: bool) -> J {
    let mut m = Map::new();
    m.insert("t".into(), json!(type_tag(v)));
    m.insert("d".into(), json!(v.display()));
    if let Some(n) = v.as_number() {
        m.insert("num".into(), json!(n));
    }
    if let Value::Date(d) = v {
        m.insert("epoch".into(), json!(d.epoch_millis()));
    }
    if semver_col {
        m.insert("sv".into(), json!(semver::column_sort_key(v)));
    }
    // Facet tokens only for lists (scalars default to `[d]` island-side). An empty
    // list omits `f` (island → "(empty)").
    if let Value::List(items) = v {
        if !items.is_empty() {
            let tokens: Vec<String> = items.iter().map(Value::display).collect();
            m.insert("f".into(), json!(tokens));
        }
    }
    if v.is_empty() {
        m.insert("empty".into(), json!(true));
    }
    J::Object(m)
}

/// Columns that stand in for the note's title: always sortable + text filter.
fn is_title_column(key: &str) -> bool {
    matches!(
        key,
        "file.name" | "file.basename" | "file.file" | "file.path"
    )
}

/// Inferred, override-resolved column metadata.
struct ColMeta {
    type_: &'static str,
    sortable: bool,
    filter: &'static str,
}

/// Infer a column's dominant type + default widget from its non-empty cells,
/// then apply any per-view overrides.
fn infer_column(
    key: &str,
    rows: &[RowView],
    max_enum: usize,
    overrides: Option<&ViewInteractive>,
) -> ColMeta {
    let non_empty: Vec<&Value> = rows
        .iter()
        .filter_map(|r| r.cells.get(key))
        .filter(|v| !v.is_empty())
        .collect();

    // Title columns are text-searchable by default regardless of content.
    let (type_, mut sortable, mut filter) = if is_title_column(key) {
        ("str", true, "text")
    } else if non_empty.is_empty() {
        // No data to infer from: a plain, sortable, no-widget string column.
        ("str", true, "none")
    } else {
        infer_from_cells(&non_empty, max_enum)
    };

    // Apply per-view overrides (explicit > auto).
    if let Some(iv) = overrides {
        if let Some(w) = iv.filters.get(key) {
            filter = normalize_widget(w).unwrap_or(filter);
        }
        if let Some(&s) = iv.sortable.get(key) {
            sortable = s;
        }
    }
    ColMeta {
        type_,
        sortable,
        filter,
    }
}

/// Core type inference over a column's non-empty cells.
fn infer_from_cells(non_empty: &[&Value], max_enum: usize) -> (&'static str, bool, &'static str) {
    let tags: BTreeSet<&'static str> = non_empty.iter().map(|v| type_tag(v)).collect();

    let type_: &'static str = if tags.len() == 1 {
        // All the same concrete type.
        tags.iter().next().copied().unwrap()
    } else if non_empty.iter().all(|v| v.as_number().is_some()) {
        // Mixed but all numeric-coercible.
        "num"
    } else {
        "str"
    };

    match type_ {
        "date" => ("date", true, "date"),
        "num" | "dur" => (type_, true, "number"),
        "bool" => ("bool", true, "boolean"),
        "list" => {
            // Multi-value column: enum over the distinct item tokens, but cap
            // cardinality like scalars so a high-cardinality list (e.g. free-form
            // tags across thousands of notes) falls back to text-search coverage
            // rather than emitting thousands of facet checkboxes.
            let mut tokens: BTreeSet<String> = BTreeSet::new();
            for v in non_empty {
                if let Value::List(items) = v {
                    for item in items {
                        tokens.insert(item.display());
                    }
                } else {
                    tokens.insert(v.display());
                }
            }
            let filter = if tokens.len() <= max_enum {
                "enum"
            } else {
                "text"
            };
            ("list", true, filter)
        }
        "obj" => ("obj", true, "none"),
        // "str" | "link" (and any fallback): enum if low-cardinality else text.
        _ => {
            let distinct: BTreeSet<String> = non_empty.iter().map(|v| v.display()).collect();
            let filter = if distinct.len() <= max_enum {
                "enum"
            } else {
                "text"
            };
            (type_, true, filter)
        }
    }
}

/// Normalize a user-supplied widget name to a known token.
fn normalize_widget(w: &str) -> Option<&'static str> {
    match w.trim().to_ascii_lowercase().as_str() {
        "none" => Some("none"),
        "text" => Some("text"),
        "enum" => Some("enum"),
        "date" => Some("date"),
        "number" => Some("number"),
        "boolean" => Some("boolean"),
        _ => None,
    }
}

/// Build the interactive payload JSON string, safe to embed inside a
/// `<script type="application/json">` element. Every `<` is escaped to its JSON
/// `<` form: this neutralizes `</script`, `<!--`, and `<script` sequences —
/// all of which can otherwise steer the HTML tokenizer's script-data states and
/// prevent the element from closing (escaping only `</` is NOT sufficient because
/// `<!--<script>` contains no `</`). `<` is valid JSON and `JSON.parse`
/// restores it to `<`, so the island sees the original strings.
pub(crate) fn build_payload(
    view: &View,
    base: &BaseFile,
    columns: &[String],
    rows: &[RowView],
    _opts: &RenderOptions,
) -> String {
    let overrides = view.interactive.as_ref();
    let max_enum = overrides
        .and_then(|iv| iv.max_enum)
        .unwrap_or(DEFAULT_MAX_ENUM);

    // columns[]
    let cols_json: Vec<J> = columns
        .iter()
        .map(|key| {
            let meta = infer_column(key, rows, max_enum, overrides);
            json!({
                "key": key,
                "header": column_header(key, base),
                "type": meta.type_,
                "sortable": meta.sortable,
                "filter": meta.filter,
            })
        })
        .collect();

    // Which columns order as versions. Decided per column (not per sort key)
    // because the island can sort by any sortable column, and once per column
    // rather than per cell because detection reads every value.
    let semver_cols: BTreeSet<&String> = columns
        .iter()
        .filter(|key| {
            render::sorts_as_semver(
                view,
                key,
                rows.iter()
                    .map(|r| r.cells.get(*key).unwrap_or(&Value::Null)),
            )
        })
        .collect();

    // rows[]
    let rows_json: Vec<J> = rows
        .iter()
        .map(|r| {
            let mut cells = Map::new();
            for key in columns {
                let v = r.cells.get(key).cloned().unwrap_or(Value::Null);
                cells.insert(key.clone(), project_cell(&v, semver_cols.contains(key)));
            }
            json!({ "id": r.id, "cells": J::Object(cells) })
        })
        .collect();

    // view.groupBy
    let group_by = view
        .group_by
        .as_ref()
        .map(|gb| json!({ "col": gb.property(), "desc": gb.descending() }));

    // controls.sort — override defaultSort wins over view.sort.
    let sort_keys = overrides
        .filter(|iv| !iv.default_sort.is_empty())
        .map(|iv| iv.default_sort.as_slice())
        .unwrap_or(view.sort.as_slice());
    let sort_json: Vec<J> = sort_keys
        .iter()
        .map(|k| json!({ "col": k.property(), "desc": k.descending() }))
        .collect();

    // controls.search — override else default true.
    let search = overrides.and_then(|iv| iv.search).unwrap_or(true);

    // controls.pageSize — override → view.limit → default 50 when >50 rows else 0.
    let page_size = overrides
        .and_then(|iv| iv.page_size)
        .or(view.limit)
        .unwrap_or(if rows.len() > 50 { 50 } else { 0 });

    let payload = json!({
        "v": 1,
        "view": {
            "type": view.view_type,
            "name": view.name,
            "groupBy": group_by,
            "limit": view.limit,
        },
        "columns": cols_json,
        "rows": rows_json,
        "controls": {
            "search": search,
            "sort": sort_json,
            "pageSize": page_size,
        },
    });

    // serde_json never fails to serialize a Value. Escape EVERY `<` (not just
    // `</`) so no `</script`/`<!--`/`<script` sequence can escape the enclosing
    // <script> element; `<` is valid JSON and parses back to `<`.
    serde_json::to_string(&payload)
        .unwrap_or_else(|_| "{}".into())
        .replace('<', "\\u003c")
}
