//! Serde model for a `.base` YAML file. Deserialization is deliberately tolerant:
//! unknown keys are ignored, every section is optional, and the filter tree
//! accepts either bare expression strings or nested `and`/`or`/`not` maps.

use std::collections::BTreeMap;

use serde::Deserialize;

/// A parsed `.base` file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BaseFile {
    /// Global filter tree applied to every view.
    pub filters: Option<Filter>,
    /// Named formulas: name → expression string.
    pub formulas: BTreeMap<String, String>,
    /// Per-property display configuration.
    pub properties: BTreeMap<String, PropertyConfig>,
    /// Named custom summary expressions.
    pub summaries: BTreeMap<String, String>,
    /// The views to render, in order.
    pub views: Vec<View>,
    /// docgen-specific (Obsidian-ignored): a bare bool that enables/disables the
    /// interactive island for the whole base. `false` = force pure static.
    #[serde(rename = "docgenInteractive")]
    pub docgen_interactive: Option<InteractiveToggle>,
}

/// A bare boolean toggle (`docgenInteractive: false`) at the base level.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
pub struct InteractiveToggle(pub bool);

impl InteractiveToggle {
    pub fn enabled(&self) -> bool {
        self.0
    }
}

/// docgen-specific per-view interactive overrides (`docgenInteractive: { ... }`).
/// Everything is optional and Obsidian-tolerant (unknown keys ignored).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ViewInteractive {
    /// Explicitly enable/disable the island for this view (M3 host gating).
    pub enabled: Option<bool>,
    /// Show the free-text search box (default: true).
    pub search: Option<bool>,
    /// Rows per page (`pageSize`). 0 = no pagination.
    #[serde(rename = "pageSize")]
    pub page_size: Option<usize>,
    /// Enum-vs-text cardinality threshold (`maxEnum`, default 40).
    #[serde(rename = "maxEnum")]
    pub max_enum: Option<usize>,
    /// Per-column filter widget override: col → `none|text|enum|date|number|boolean`.
    pub filters: BTreeMap<String, String>,
    /// Per-column sortable override: col → bool.
    pub sortable: BTreeMap<String, bool>,
    /// Initial sort override (`defaultSort`).
    #[serde(rename = "defaultSort")]
    pub default_sort: Vec<SortKey>,
}

/// Per-property display config (`properties.<key>.displayName`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PropertyConfig {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

/// A filter node: a leaf expression string, or a logical combinator over
/// sub-filters. Obsidian writes these as `- expr` list items and
/// `and:`/`or:`/`not:` maps.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Filter {
    /// A single expression string (e.g. `file.hasTag("book")`).
    Expr(String),
    /// A logical combinator. Exactly one of `and`/`or`/`not` is expected, but the
    /// struct tolerates any subset (missing = absent).
    Logic(LogicFilter),
    /// A bare list of filters is treated as an implicit `and`.
    List(Vec<Filter>),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LogicFilter {
    pub and: Option<Vec<Filter>>,
    pub or: Option<Vec<Filter>>,
    /// `not` accepts either a single filter or a list (all negated & AND-ed).
    pub not: Option<Box<NotFilter>>,
}

/// `not:` may hold a single filter or a list of them.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum NotFilter {
    One(Filter),
    Many(Vec<Filter>),
}

/// A single view configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct View {
    /// `table` (default), `cards`, or `list`.
    #[serde(rename = "type")]
    pub view_type: String,
    /// Display name (view tab label / section heading).
    pub name: Option<String>,
    /// View-level filter, AND-combined with the global filter.
    pub filters: Option<Filter>,
    /// Columns to show and their order (property references). Empty = infer from
    /// the data (all note properties seen, plus `file.name`).
    pub order: Vec<String>,
    /// Row sort keys, applied in order.
    pub sort: Vec<SortKey>,
    /// Grouping (rows partitioned by this property).
    #[serde(rename = "groupBy")]
    pub group_by: Option<GroupBy>,
    /// Row cap.
    pub limit: Option<usize>,
    /// Per-column pixel widths (`columnSize.<prop> = 381`).
    #[serde(rename = "columnSize")]
    pub column_size: BTreeMap<String, u32>,
    /// Per-column summary function name (`summaries.<prop> = Average`).
    pub summaries: BTreeMap<String, String>,
    /// Cards view: property whose value (image/url) is the card cover.
    pub image: Option<String>,
    /// docgen-specific (Obsidian-ignored) interactive overrides for this view.
    #[serde(rename = "docgenInteractive")]
    pub interactive: Option<ViewInteractive>,
}

/// A sort key: which property, ascending or descending.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SortKey {
    /// Shorthand: just a property name (ascending).
    Property(String),
    /// Full form: `{ property: file.name, direction: DESC }`.
    Full {
        property: String,
        #[serde(default)]
        direction: Option<String>,
    },
}

impl SortKey {
    pub fn property(&self) -> &str {
        match self {
            SortKey::Property(p) => p,
            SortKey::Full { property, .. } => property,
        }
    }

    /// True when this key sorts descending.
    pub fn descending(&self) -> bool {
        match self {
            SortKey::Property(_) => false,
            SortKey::Full { direction, .. } => direction
                .as_deref()
                .map(|d| d.eq_ignore_ascii_case("desc"))
                .unwrap_or(false),
        }
    }
}

/// `groupBy: { property, direction }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum GroupBy {
    Property(String),
    Full {
        property: String,
        #[serde(default)]
        direction: Option<String>,
    },
}

impl GroupBy {
    pub fn property(&self) -> &str {
        match self {
            GroupBy::Property(p) => p,
            GroupBy::Full { property, .. } => property,
        }
    }

    pub fn descending(&self) -> bool {
        match self {
            GroupBy::Property(_) => false,
            GroupBy::Full { direction, .. } => direction
                .as_deref()
                .map(|d| d.eq_ignore_ascii_case("desc"))
                .unwrap_or(false),
        }
    }
}

/// Parse a `.base` YAML document. Returns a detailed error on malformed YAML.
pub fn parse_base(yaml: &str) -> Result<BaseFile, serde_yml::Error> {
    // An empty document is a valid (empty) base.
    if yaml.trim().is_empty() {
        return Ok(BaseFile::default());
    }
    serde_yml::from_str(yaml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_vault_shape() {
        let yaml = r#"
filters:
  and:
    - categories.contains(link("Categories/Books", "Books"))
    - '!file.inFolder("Misc")'
views:
  - type: table
    name: Table
    order:
      - file.name
      - file.size
    sort:
      - property: file.name
        direction: DESC
    columnSize:
      file.name: 381
"#;
        let base = parse_base(yaml).unwrap();
        assert!(base.filters.is_some());
        assert_eq!(base.views.len(), 1);
        let v = &base.views[0];
        assert_eq!(v.view_type, "table");
        assert_eq!(v.name.as_deref(), Some("Table"));
        assert_eq!(v.order, vec!["file.name", "file.size"]);
        assert_eq!(v.sort.len(), 1);
        assert_eq!(v.sort[0].property(), "file.name");
        assert!(v.sort[0].descending());
        assert_eq!(v.column_size.get("file.name"), Some(&381));
    }

    #[test]
    fn parses_formulas_and_summaries() {
        let yaml = r#"
formulas:
  formatted_price: 'if(price, price.toFixed(2) + " dollars")'
  ppu: "(price / age).toFixed(2)"
summaries:
  customAverage: 'values.mean().round(3)'
properties:
  status:
    displayName: Status
views:
  - type: table
    summaries:
      formula.ppu: Average
"#;
        let base = parse_base(yaml).unwrap();
        assert_eq!(base.formulas.len(), 2);
        assert!(base.formulas.contains_key("ppu"));
        assert_eq!(
            base.summaries.get("customAverage").map(String::as_str),
            Some("values.mean().round(3)")
        );
        assert_eq!(
            base.properties
                .get("status")
                .and_then(|p| p.display_name.as_deref()),
            Some("Status")
        );
        assert_eq!(
            base.views[0]
                .summaries
                .get("formula.ppu")
                .map(String::as_str),
            Some("Average")
        );
    }

    #[test]
    fn tolerates_unknown_keys() {
        let yaml = "unknownTop: 1\nviews:\n  - type: table\n    bogusField: x\n";
        let base = parse_base(yaml).unwrap();
        assert_eq!(base.views.len(), 1);
    }

    #[test]
    fn parses_docgen_interactive_overrides() {
        let yaml = r#"
docgenInteractive: false
views:
  - type: table
    order: [file.name, note.status]
    docgenInteractive:
      enabled: true
      search: false
      pageSize: 25
      maxEnum: 10
      filters:
        note.status: text
      sortable:
        note.status: false
      defaultSort:
        - property: note.status
          direction: DESC
"#;
        let base = parse_base(yaml).unwrap();
        assert_eq!(base.docgen_interactive.map(|t| t.enabled()), Some(false));
        let v = &base.views[0];
        let iv = v.interactive.as_ref().unwrap();
        assert_eq!(iv.enabled, Some(true));
        assert_eq!(iv.search, Some(false));
        assert_eq!(iv.page_size, Some(25));
        assert_eq!(iv.max_enum, Some(10));
        assert_eq!(
            iv.filters.get("note.status").map(String::as_str),
            Some("text")
        );
        assert_eq!(iv.sortable.get("note.status"), Some(&false));
        assert_eq!(iv.default_sort.len(), 1);
        assert_eq!(iv.default_sort[0].property(), "note.status");
        assert!(iv.default_sort[0].descending());
    }

    #[test]
    fn base_without_interactive_keys_still_parses() {
        // Mirrors `tolerates_unknown_keys`: the new optional fields default to None.
        let base = parse_base("views:\n  - type: table\n    order: [file.name]\n").unwrap();
        assert!(base.docgen_interactive.is_none());
        assert!(base.views[0].interactive.is_none());
    }

    #[test]
    fn empty_base_is_valid() {
        assert_eq!(parse_base("").unwrap().views.len(), 0);
        assert_eq!(parse_base("   \n").unwrap().views.len(), 0);
    }

    #[test]
    fn shorthand_sort_and_group() {
        let yaml =
            "views:\n  - type: table\n    sort:\n      - file.name\n    groupBy: note.status\n";
        let base = parse_base(yaml).unwrap();
        assert_eq!(base.views[0].sort[0].property(), "file.name");
        assert!(!base.views[0].sort[0].descending());
        assert_eq!(
            base.views[0].group_by.as_ref().unwrap().property(),
            "note.status"
        );
    }
}
