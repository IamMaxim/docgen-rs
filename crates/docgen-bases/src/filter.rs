//! Compiles a [`Filter`] tree into a set of parsed predicate expressions and
//! evaluates it against a note. Global and view filters are AND-combined by the
//! caller. A leaf expression that fails to parse is treated as `false` (the row
//! is excluded) — surfaced separately as a diagnostic by the renderer.

use crate::ast::Expr;
use crate::eval::EvalCtx;
use crate::model::{Filter, LogicFilter, NotFilter};
use crate::parser::parse;

/// A compiled filter: a boolean tree of parsed expressions.
#[derive(Debug, Clone)]
pub enum CompiledFilter {
    /// Always true (an absent filter).
    True,
    Expr(Expr),
    /// A leaf expression that failed to parse (evaluates false; message kept).
    Invalid(String),
    And(Vec<CompiledFilter>),
    Or(Vec<CompiledFilter>),
    Not(Box<CompiledFilter>),
}

impl CompiledFilter {
    /// Evaluate the filter against a note context.
    pub fn matches(&self, ctx: &EvalCtx) -> bool {
        match self {
            CompiledFilter::True => true,
            CompiledFilter::Invalid(_) => false,
            CompiledFilter::Expr(e) => ctx.eval(e).is_truthy(),
            CompiledFilter::And(items) => items.iter().all(|f| f.matches(ctx)),
            CompiledFilter::Or(items) => items.iter().any(|f| f.matches(ctx)),
            CompiledFilter::Not(inner) => !inner.matches(ctx),
        }
    }

    /// Collect parse-error messages for diagnostics.
    pub fn errors(&self, out: &mut Vec<String>) {
        match self {
            CompiledFilter::Invalid(msg) => out.push(msg.clone()),
            CompiledFilter::And(items) | CompiledFilter::Or(items) => {
                items.iter().for_each(|f| f.errors(out))
            }
            CompiledFilter::Not(inner) => inner.errors(out),
            _ => {}
        }
    }
}

/// Compile an optional filter into a predicate tree (absent → always true).
pub fn compile(filter: &Option<Filter>) -> CompiledFilter {
    match filter {
        None => CompiledFilter::True,
        Some(f) => compile_node(f),
    }
}

/// AND-combine a global and a view filter (either may be absent).
pub fn combine(global: &Option<Filter>, view: &Option<Filter>) -> CompiledFilter {
    let parts: Vec<CompiledFilter> = [global, view]
        .into_iter()
        .filter_map(|f| f.as_ref().map(compile_node))
        .collect();
    match parts.len() {
        0 => CompiledFilter::True,
        1 => parts.into_iter().next().unwrap(),
        _ => CompiledFilter::And(parts),
    }
}

fn compile_node(f: &Filter) -> CompiledFilter {
    match f {
        Filter::Expr(s) => match parse(s) {
            Ok(e) => CompiledFilter::Expr(e),
            Err(msg) => CompiledFilter::Invalid(format!("{s}: {msg}")),
        },
        Filter::List(items) => CompiledFilter::And(items.iter().map(compile_node).collect()),
        Filter::Logic(logic) => compile_logic(logic),
    }
}

fn compile_logic(logic: &LogicFilter) -> CompiledFilter {
    let mut parts: Vec<CompiledFilter> = Vec::new();
    if let Some(and) = &logic.and {
        parts.push(CompiledFilter::And(and.iter().map(compile_node).collect()));
    }
    if let Some(or) = &logic.or {
        parts.push(CompiledFilter::Or(or.iter().map(compile_node).collect()));
    }
    if let Some(not) = &logic.not {
        let inner = match &**not {
            NotFilter::One(f) => compile_node(f),
            NotFilter::Many(fs) => CompiledFilter::And(fs.iter().map(compile_node).collect()),
        };
        parts.push(CompiledFilter::Not(Box::new(inner)));
    }
    match parts.len() {
        0 => CompiledFilter::True,
        1 => parts.into_iter().next().unwrap(),
        _ => CompiledFilter::And(parts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse_base;
    use crate::note::{Corpus, Note};
    use crate::value::{BaseLink, Value};
    use std::collections::BTreeMap;

    fn matches(filter: &Option<Filter>, note: &Note) -> bool {
        let corpus = Corpus::new(vec![note.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(note, &corpus, &formulas);
        compile(filter).matches(&ctx)
    }

    fn note_in_folder(folder: &str, cats: Vec<&str>) -> Note {
        let mut n = Note::default();
        n.folder = folder.to_string();
        n.properties.insert(
            "categories".into(),
            Value::List(
                cats.into_iter()
                    .map(|c| Value::Link(BaseLink::new(c)))
                    .collect(),
            ),
        );
        n
    }

    #[test]
    fn and_with_negation() {
        let base = parse_base(
            "filters:\n  and:\n    - categories.contains(link(\"Books\"))\n    - '!file.inFolder(\"Misc\")'\n",
        )
        .unwrap();
        // In Books, not in Misc → included.
        assert!(matches(
            &base.filters,
            &note_in_folder("Library", vec!["Books"])
        ));
        // In Misc → excluded.
        assert!(!matches(
            &base.filters,
            &note_in_folder("Misc", vec!["Books"])
        ));
        // Not in Books → excluded.
        assert!(!matches(
            &base.filters,
            &note_in_folder("Library", vec!["Films"])
        ));
    }

    #[test]
    fn or_combinator() {
        let base =
            parse_base("filters:\n  or:\n    - file.hasTag(\"a\")\n    - file.hasTag(\"b\")\n")
                .unwrap();
        let mut n = Note::default();
        n.tags = vec!["b".into()];
        assert!(matches(&base.filters, &n));
        n.tags = vec!["c".into()];
        assert!(!matches(&base.filters, &n));
    }

    #[test]
    fn combine_global_and_view() {
        let base = parse_base("filters:\n  and:\n    - file.hasTag(\"x\")\n").unwrap();
        let view_filter: Option<Filter> = Some(Filter::Expr("file.hasTag(\"y\")".into()));
        let combined = combine(&base.filters, &view_filter);
        let mut n = Note::default();
        n.tags = vec!["x".into(), "y".into()];
        let corpus = Corpus::new(vec![n.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(&n, &corpus, &formulas);
        assert!(combined.matches(&ctx));
        // Missing y → excluded.
        let mut n2 = Note::default();
        n2.tags = vec!["x".into()];
        let corpus2 = Corpus::new(vec![n2.clone()]);
        let ctx2 = EvalCtx::new(&n2, &corpus2, &formulas);
        assert!(!combined.matches(&ctx2));
    }

    #[test]
    fn invalid_expression_excludes_and_reports() {
        let f = Some(Filter::Expr("this is ( not valid".into()));
        let compiled = compile(&f);
        let mut errs = Vec::new();
        compiled.errors(&mut errs);
        assert_eq!(errs.len(), 1);
        assert!(!matches(&f, &Note::default()));
    }
}
