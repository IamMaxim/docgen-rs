//! Column summary functions for a table's footer row. Built-in summaries operate
//! over the set of a column's cell values; a custom summary (defined in the
//! base's `summaries:` map) is an expression evaluated with `values` bound to the
//! column's list of values.

use std::collections::BTreeMap;

use crate::ast::Expr;
use crate::eval::EvalCtx;
use crate::note::{Corpus, Note};
use crate::value::{format_number, BaseDate, Value};

/// Compute a summary for a column of `values` using the summary named `name`.
/// `name` is either a built-in (case-insensitive) or a key into `custom`
/// (the base's `summaries:` map, pre-parsed). Returns the rendered footer string.
pub fn summarize(
    name: &str,
    values: &[Value],
    custom: &BTreeMap<String, Expr>,
    corpus: &Corpus,
    formulas: &BTreeMap<String, Expr>,
) -> String {
    // Custom summary: evaluate its expression with `values` bound.
    if let Some(expr) = custom.get(name) {
        let mut note = Note::default();
        note.properties
            .insert("values".to_string(), Value::List(values.to_vec()));
        let ctx = EvalCtx::new(&note, corpus, formulas);
        return ctx.eval(expr).display();
    }

    let nums: Vec<f64> = values.iter().filter_map(Value::as_number).collect();
    let dates: Vec<BaseDate> = values
        .iter()
        .filter_map(|v| match v {
            Value::Date(d) => Some(*d),
            _ => None,
        })
        .collect();

    match name.to_ascii_lowercase().as_str() {
        // Universal.
        "count" => values.len().to_string(),
        "empty" => values.iter().filter(|v| v.is_empty()).count().to_string(),
        "filled" | "notempty" => values.iter().filter(|v| !v.is_empty()).count().to_string(),
        "unique" => {
            let mut seen: Vec<&Value> = Vec::new();
            for v in values {
                if !seen.iter().any(|s| s.loose_eq(v)) {
                    seen.push(v);
                }
            }
            seen.len().to_string()
        }
        // Numeric.
        "sum" => opt_num(if nums.is_empty() {
            None
        } else {
            Some(nums.iter().sum())
        }),
        "average" | "mean" | "avg" => opt_num(if nums.is_empty() {
            None
        } else {
            Some(nums.iter().sum::<f64>() / nums.len() as f64)
        }),
        "min" => opt_num(nums.iter().cloned().reduce(f64::min)),
        "max" => opt_num(nums.iter().cloned().reduce(f64::max)),
        "median" => opt_num(median(&nums)),
        "range" => opt_num(
            match (
                nums.iter().cloned().reduce(f64::min),
                nums.iter().cloned().reduce(f64::max),
            ) {
                (Some(lo), Some(hi)) => Some(hi - lo),
                _ => None,
            },
        ),
        "stddev" | "std" => opt_num(stddev(&nums)),
        // Boolean.
        "checked" => values
            .iter()
            .filter(|v| matches!(v, Value::Bool(true)))
            .count()
            .to_string(),
        "unchecked" => values
            .iter()
            .filter(|v| matches!(v, Value::Bool(false)))
            .count()
            .to_string(),
        // Dates.
        "earliest" => dates
            .iter()
            .min_by_key(|d| d.epoch_millis())
            .map(crate::format::default_date)
            .unwrap_or_default(),
        "latest" => dates
            .iter()
            .max_by_key(|d| d.epoch_millis())
            .map(crate::format::default_date)
            .unwrap_or_default(),
        // Unknown / "none".
        "none" | "" => String::new(),
        _ => String::new(),
    }
}

fn opt_num(v: Option<f64>) -> String {
    v.map(format_number).unwrap_or_default()
}

fn median(nums: &[f64]) -> Option<f64> {
    if nums.is_empty() {
        return None;
    }
    let mut sorted = nums.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    })
}

fn stddev(nums: &[f64]) -> Option<f64> {
    if nums.len() < 2 {
        return None;
    }
    let mean = nums.iter().sum::<f64>() / nums.len() as f64;
    let var = nums.iter().map(|n| (n - mean).powi(2)).sum::<f64>() / nums.len() as f64;
    Some(var.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn nums(v: &[f64]) -> Vec<Value> {
        v.iter().cloned().map(Value::Number).collect()
    }

    fn empty_custom() -> BTreeMap<String, Expr> {
        BTreeMap::new()
    }

    #[test]
    fn numeric_summaries() {
        let vals = nums(&[1.0, 2.0, 3.0, 4.0]);
        let c = Corpus::default();
        let f = BTreeMap::new();
        assert_eq!(summarize("Sum", &vals, &empty_custom(), &c, &f), "10");
        assert_eq!(summarize("Average", &vals, &empty_custom(), &c, &f), "2.5");
        assert_eq!(summarize("Min", &vals, &empty_custom(), &c, &f), "1");
        assert_eq!(summarize("Max", &vals, &empty_custom(), &c, &f), "4");
        assert_eq!(summarize("Median", &vals, &empty_custom(), &c, &f), "2.5");
        assert_eq!(summarize("Range", &vals, &empty_custom(), &c, &f), "3");
    }

    #[test]
    fn universal_summaries() {
        let vals = vec![
            Value::Str("a".into()),
            Value::Null,
            Value::Str("a".into()),
            Value::Str("b".into()),
        ];
        let c = Corpus::default();
        let f = BTreeMap::new();
        assert_eq!(summarize("Count", &vals, &empty_custom(), &c, &f), "4");
        assert_eq!(summarize("Empty", &vals, &empty_custom(), &c, &f), "1");
        assert_eq!(summarize("Filled", &vals, &empty_custom(), &c, &f), "3");
        assert_eq!(summarize("Unique", &vals, &empty_custom(), &c, &f), "3");
    }

    #[test]
    fn boolean_summaries() {
        let vals = vec![Value::Bool(true), Value::Bool(false), Value::Bool(true)];
        let c = Corpus::default();
        let f = BTreeMap::new();
        assert_eq!(summarize("Checked", &vals, &empty_custom(), &c, &f), "2");
        assert_eq!(summarize("Unchecked", &vals, &empty_custom(), &c, &f), "1");
    }

    #[test]
    fn custom_summary_uses_values() {
        let vals = nums(&[1.0, 2.0, 3.0]);
        let mut custom = BTreeMap::new();
        custom.insert(
            "customAverage".to_string(),
            parse("values.mean().round(3)").unwrap(),
        );
        let c = Corpus::default();
        let f = BTreeMap::new();
        assert_eq!(summarize("customAverage", &vals, &custom, &c, &f), "2");
    }

    #[test]
    fn date_summaries() {
        let vals = vec![
            Value::Date(crate::note::parse_date("2024-01-05").unwrap()),
            Value::Date(crate::note::parse_date("2024-01-01").unwrap()),
        ];
        let c = Corpus::default();
        let f = BTreeMap::new();
        assert_eq!(
            summarize("Earliest", &vals, &empty_custom(), &c, &f),
            "2024-01-01"
        );
        assert_eq!(
            summarize("Latest", &vals, &empty_custom(), &c, &f),
            "2024-01-05"
        );
    }
}
