//! The Bases function library: global functions (`link`, `date`, `if`, `list`,
//! …) and per-type methods (String/Number/Date/List/Link/File/Object/Any). All
//! dispatch is name-based and total — an unknown name or a type mismatch yields
//! [`Value::Null`], never a panic.

use crate::ast::Expr;
use crate::eval::{add_duration, value_member, EvalCtx};
use crate::note::{parse_date, Note};
use crate::value::{BaseDate, BaseLink, Value};

/// Evaluate each argument expression eagerly.
fn eval_args(args: &[Expr], ctx: &EvalCtx) -> Vec<Value> {
    args.iter().map(|a| ctx.eval(a)).collect()
}

/// Dispatch a global function call `name(args)`.
pub fn global_call(name: &str, args: &[Expr], ctx: &EvalCtx) -> Value {
    match name {
        // List literal sugar from the parser (`[a, b]`).
        "__list" | "list" => {
            let vals = eval_args(args, ctx);
            if name == "list" && vals.len() == 1 {
                // list(x): wrap a scalar, or pass a list through unchanged.
                return match vals.into_iter().next().unwrap() {
                    v @ Value::List(_) => v,
                    other => Value::List(vec![other]),
                };
            }
            Value::List(vals)
        }
        "if" => {
            // Lazy in the branches: only evaluate the taken branch.
            if args.is_empty() {
                return Value::Null;
            }
            let cond = ctx.eval(&args[0]);
            if cond.is_truthy() {
                args.get(1).map(|e| ctx.eval(e)).unwrap_or(Value::Null)
            } else {
                args.get(2).map(|e| ctx.eval(e)).unwrap_or(Value::Null)
            }
        }
        "link" => {
            let vals = eval_args(args, ctx);
            let path = vals.first().map(coerce_link_path).unwrap_or_default();
            match vals.get(1) {
                Some(d) => Value::Link(BaseLink::with_display(path, d.display())),
                None => Value::Link(BaseLink::new(path)),
            }
        }
        "date" => {
            let vals = eval_args(args, ctx);
            match vals.into_iter().next() {
                Some(Value::Date(d)) => Value::Date(d),
                Some(Value::Str(s)) => parse_date(&s).map(Value::Date).unwrap_or(Value::Null),
                _ => Value::Null,
            }
        }
        "now" => Value::Null, // build-time "now" is intentionally inert (determinism)
        "today" => Value::Null,
        "duration" => {
            let vals = eval_args(args, ctx);
            match vals.into_iter().next() {
                Some(Value::Str(s)) => parse_duration(&s)
                    .map(Value::Duration)
                    .unwrap_or(Value::Null),
                Some(Value::Number(n)) => Value::Duration(n as i64),
                Some(Value::Duration(d)) => Value::Duration(d),
                _ => Value::Null,
            }
        }
        "number" => {
            let vals = eval_args(args, ctx);
            vals.first()
                .and_then(Value::as_number)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        "min" => numeric_reduce(&eval_args(args, ctx), f64::min),
        "max" => numeric_reduce(&eval_args(args, ctx), f64::max),
        "max_or_null" => numeric_reduce(&eval_args(args, ctx), f64::max),
        "random" => Value::Number(0.0), // deterministic build: no RNG
        // Rendering helpers: at build time we keep the string; the renderer
        // escapes as needed. `html`/`image`/`icon` degrade to their text.
        "html" | "image" | "icon" | "escapeHTML" => eval_args(args, ctx)
            .into_iter()
            .next()
            .unwrap_or(Value::Null),
        "file" => {
            // file(path): resolve to a link (the closest static analogue).
            let vals = eval_args(args, ctx);
            vals.first()
                .map(|v| Value::Link(BaseLink::new(coerce_link_path(v))))
                .unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

/// A `link(x)` argument may be a string path or an existing link.
fn coerce_link_path(v: &Value) -> String {
    match v {
        Value::Link(l) => l.path.clone(),
        Value::Str(s) => {
            // Accept a `[[wikilink]]` string too.
            crate::note::parse_wikilink(s.trim())
                .map(|l| l.path)
                .unwrap_or_else(|| s.clone())
        }
        other => other.display(),
    }
}

fn numeric_reduce(vals: &[Value], f: impl Fn(f64, f64) -> f64) -> Value {
    // Flatten a single list argument (min([a,b,c])) or variadic numbers.
    let nums: Vec<f64> = if vals.len() == 1 {
        match &vals[0] {
            Value::List(items) => items.iter().filter_map(Value::as_number).collect(),
            v => v.as_number().into_iter().collect(),
        }
    } else {
        vals.iter().filter_map(Value::as_number).collect()
    };
    match nums.split_first() {
        Some((first, rest)) => Value::Number(rest.iter().fold(*first, |a, b| f(a, *b))),
        None => Value::Null,
    }
}

/// Dispatch a `file.<method>(args)` call against `note`.
pub fn file_method(note: &Note, method: &str, args: &[Expr], ctx: &EvalCtx) -> Value {
    let vals = eval_args(args, ctx);
    match method {
        "hasTag" => Value::Bool(vals.iter().any(|v| note.has_tag(&v.display()))),
        "hasProperty" => Value::Bool(
            vals.first()
                .map(|v| note.properties.contains_key(&v.display()))
                .unwrap_or(false),
        ),
        "inFolder" => Value::Bool(
            vals.first()
                .map(|v| note.in_folder(&v.display()))
                .unwrap_or(false),
        ),
        "hasLink" => Value::Bool(vals.iter().any(|v| {
            let target = match v {
                Value::Link(l) => l.clone(),
                other => BaseLink::new(coerce_link_path(other)),
            };
            note.has_link(&target)
        })),
        "asLink" => {
            let display = vals.first().map(Value::display);
            Value::Link(match display {
                Some(d) if !d.is_empty() => BaseLink::with_display(note.path.clone(), d),
                _ => BaseLink::new(note.basename.clone()),
            })
        }
        // Otherwise treat as a property access (`file.tags` reached via method-ish).
        _ => {
            let _ = ctx;
            note.file_property(method)
        }
    }
}

/// Dispatch a `<value>.<method>(args)` call.
pub fn method_call(recv: &Value, method: &str, args: &[Expr], ctx: &EvalCtx) -> Value {
    // Methods that need lazy arg exprs (map/filter) are handled first.
    match method {
        "map" => return list_map(recv, args, ctx),
        "filter" => return list_filter(recv, args, ctx),
        _ => {}
    }
    let vals = eval_args(args, ctx);
    match recv {
        Value::Str(s) => string_method(s, method, &vals),
        Value::Number(n) => number_method(*n, method, &vals),
        Value::Date(d) => date_method(d, method, &vals),
        Value::Duration(_) => any_method(recv, method, &vals),
        Value::List(items) => list_method(items, method, &vals),
        Value::Object(map) => object_method(map, method, &vals),
        Value::Link(l) => link_method(l, method, &vals, ctx),
        Value::Bool(_) | Value::Null => any_method(recv, method, &vals),
    }
    // Fall back to a plain member (e.g. `.length`) if the method wasn't matched
    // returns Null; callers already treat Null gracefully.
}

/// Methods available on any value.
fn any_method(recv: &Value, method: &str, _args: &[Value]) -> Value {
    match method {
        "isEmpty" => Value::Bool(recv.is_empty()),
        "isTruthy" => Value::Bool(recv.is_truthy()),
        "toString" => Value::Str(recv.display()),
        "isType" => Value::Bool(
            _args
                .first()
                .map(|a| a.display() == recv.type_name())
                .unwrap_or(false),
        ),
        _ => Value::Null,
    }
}

fn string_method(s: &str, method: &str, args: &[Value]) -> Value {
    let arg0 = args.first().map(Value::display).unwrap_or_default();
    match method {
        "contains" => Value::Bool(s.contains(&arg0)),
        "containsAll" => Value::Bool(args.iter().all(|a| s.contains(&a.display()))),
        "containsAny" => Value::Bool(args.iter().any(|a| s.contains(&a.display()))),
        "startsWith" => Value::Bool(s.starts_with(&arg0)),
        "endsWith" => Value::Bool(s.ends_with(&arg0)),
        "isEmpty" => Value::Bool(s.is_empty()),
        "lower" => Value::Str(s.to_lowercase()),
        "upper" => Value::Str(s.to_uppercase()),
        "trim" => Value::Str(s.trim().to_string()),
        "title" => Value::Str(title_case(s)),
        "reverse" => Value::Str(s.chars().rev().collect()),
        "length" => Value::Number(s.chars().count() as f64),
        "repeat" => {
            let n = args
                .first()
                .and_then(Value::as_number)
                .unwrap_or(0.0)
                .max(0.0) as usize;
            Value::Str(s.repeat(n))
        }
        "replace" => {
            let to = args.get(1).map(Value::display).unwrap_or_default();
            Value::Str(s.replace(&arg0, &to))
        }
        "slice" => {
            let start = args.first().and_then(Value::as_number).unwrap_or(0.0) as i64;
            let end = args.get(1).and_then(Value::as_number).map(|n| n as i64);
            Value::Str(slice_chars(s, start, end))
        }
        "split" => {
            let parts: Vec<Value> = if arg0.is_empty() {
                s.chars().map(|c| Value::Str(c.to_string())).collect()
            } else {
                s.split(&arg0).map(|p| Value::Str(p.to_string())).collect()
            };
            let limited = match args.get(1).and_then(Value::as_number) {
                Some(n) => parts.into_iter().take(n as usize).collect(),
                None => parts,
            };
            Value::List(limited)
        }
        "toNumber" => arg0
            .parse::<f64>()
            .map(Value::Number)
            .unwrap_or(Value::Null),
        _ => any_method(&Value::Str(s.to_string()), method, args),
    }
}

fn number_method(n: f64, method: &str, args: &[Value]) -> Value {
    match method {
        "abs" => Value::Number(n.abs()),
        "ceil" => Value::Number(n.ceil()),
        "floor" => Value::Number(n.floor()),
        "round" => {
            let digits = args.first().and_then(Value::as_number).unwrap_or(0.0) as i32;
            let f = 10f64.powi(digits);
            Value::Number((n * f).round() / f)
        }
        "toFixed" => {
            let p = args.first().and_then(Value::as_number).unwrap_or(0.0) as usize;
            Value::Str(format!("{n:.*}", p))
        }
        "isEmpty" => Value::Bool(false),
        _ => any_method(&Value::Number(n), method, args),
    }
}

fn date_method(d: &BaseDate, method: &str, args: &[Value]) -> Value {
    match method {
        "format" => {
            let fmt = args.first().map(Value::display).unwrap_or_default();
            if fmt.is_empty() {
                Value::Str(crate::format::default_date(d))
            } else {
                Value::Str(crate::format::format(d, &fmt))
            }
        }
        "date" => {
            let mut bare = *d;
            bare.hour = 0;
            bare.minute = 0;
            bare.second = 0;
            bare.millisecond = 0;
            bare.has_time = false;
            Value::Date(bare)
        }
        "time" => Value::Str(crate::format::format(d, "HH:mm:ss")),
        "isEmpty" => Value::Bool(false),
        "year" => Value::Number(d.year as f64),
        "month" => Value::Number(d.month as f64),
        "day" => Value::Number(d.day as f64),
        "hour" => Value::Number(d.hour as f64),
        "minute" => Value::Number(d.minute as f64),
        "second" => Value::Number(d.second as f64),
        "plus" | "add" => {
            // date.plus(duration)
            match args.first() {
                Some(Value::Duration(ms)) => Value::Date(add_duration(d, *ms)),
                Some(Value::Str(s)) => parse_duration(s)
                    .map(|ms| Value::Date(add_duration(d, ms)))
                    .unwrap_or(Value::Date(*d)),
                _ => Value::Date(*d),
            }
        }
        _ => any_method(&Value::Date(*d), method, args),
    }
}

fn list_method(items: &[Value], method: &str, args: &[Value]) -> Value {
    let arg0 = args.first();
    match method {
        "contains" => Value::Bool(
            arg0.map(|a| items.iter().any(|x| x.loose_eq(a)))
                .unwrap_or(false),
        ),
        "containsAll" => Value::Bool(args.iter().all(|a| items.iter().any(|x| x.loose_eq(a)))),
        "containsAny" => Value::Bool(args.iter().any(|a| items.iter().any(|x| x.loose_eq(a)))),
        "isEmpty" => Value::Bool(items.is_empty()),
        "length" => Value::Number(items.len() as f64),
        "join" => {
            let sep = arg0.map(Value::display).unwrap_or_default();
            Value::Str(
                items
                    .iter()
                    .map(Value::display)
                    .collect::<Vec<_>>()
                    .join(&sep),
            )
        }
        "reverse" => Value::List(items.iter().rev().cloned().collect()),
        "sort" => {
            let mut v = items.to_vec();
            v.sort_by(|a, b| a.loose_cmp(b));
            Value::List(v)
        }
        "unique" => {
            let mut out: Vec<Value> = Vec::new();
            for it in items {
                if !out.iter().any(|x| x.loose_eq(it)) {
                    out.push(it.clone());
                }
            }
            Value::List(out)
        }
        "flat" => {
            let mut out = Vec::new();
            for it in items {
                match it {
                    Value::List(inner) => out.extend(inner.iter().cloned()),
                    other => out.push(other.clone()),
                }
            }
            Value::List(out)
        }
        "slice" => {
            let start = arg0.and_then(Value::as_number).unwrap_or(0.0) as i64;
            let end = args.get(1).and_then(Value::as_number).map(|n| n as i64);
            Value::List(slice_vec(items, start, end))
        }
        "first" => items.first().cloned().unwrap_or(Value::Null),
        "last" => items.last().cloned().unwrap_or(Value::Null),
        // Aggregations usable in formulas/summaries.
        "sum" => Value::Number(items.iter().filter_map(Value::as_number).sum()),
        "average" | "mean" => {
            let nums: Vec<f64> = items.iter().filter_map(Value::as_number).collect();
            if nums.is_empty() {
                Value::Null
            } else {
                Value::Number(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        }
        "min" => numeric_reduce(&[Value::List(items.to_vec())], f64::min),
        "max" => numeric_reduce(&[Value::List(items.to_vec())], f64::max),
        _ => any_method(&Value::List(items.to_vec()), method, args),
    }
}

fn object_method(
    map: &std::collections::BTreeMap<String, Value>,
    method: &str,
    args: &[Value],
) -> Value {
    match method {
        "isEmpty" => Value::Bool(map.is_empty()),
        "keys" => Value::List(map.keys().cloned().map(Value::Str).collect()),
        "values" => Value::List(map.values().cloned().collect()),
        _ => any_method(&Value::Object(map.clone()), method, args),
    }
}

fn link_method(l: &BaseLink, method: &str, args: &[Value], ctx: &EvalCtx) -> Value {
    match method {
        "linksTo" => {
            // Does the note referenced by this link link to the argument file?
            let target = args.first().map(|v| BaseLink::new(coerce_link_path(v)));
            match target {
                Some(t) => {
                    // Find the note this link points at, check its outbound links.
                    let found = ctx
                        .corpus
                        .notes
                        .iter()
                        .find(|n| n.basename.eq_ignore_ascii_case(l.basename()));
                    Value::Bool(found.map(|n| n.has_link(&t)).unwrap_or(false))
                }
                None => Value::Bool(false),
            }
        }
        "asFile" | "asLink" => Value::Link(l.clone()),
        "path" => Value::Str(l.path.clone()),
        "display" => l.display.clone().map(Value::Str).unwrap_or(Value::Null),
        _ => value_member(&Value::Link(l.clone()), method),
    }
}

// --- list map/filter (need the raw arg expr as a per-element predicate) -------

/// `list.map(expr)` — evaluate `expr` with the special identifier `value` bound
/// to each element.
fn list_map(recv: &Value, args: &[Expr], ctx: &EvalCtx) -> Value {
    let Value::List(items) = recv else {
        return Value::Null;
    };
    let Some(body) = args.first() else {
        return recv.clone();
    };
    Value::List(
        items
            .iter()
            .map(|it| eval_with_value(body, it, ctx))
            .collect(),
    )
}

/// `list.filter(expr)` — keep elements for which `expr` (with `value` bound) is
/// truthy.
fn list_filter(recv: &Value, args: &[Expr], ctx: &EvalCtx) -> Value {
    let Value::List(items) = recv else {
        return Value::Null;
    };
    let Some(pred) = args.first() else {
        return recv.clone();
    };
    Value::List(
        items
            .iter()
            .filter(|it| eval_with_value(pred, it, ctx).is_truthy())
            .cloned()
            .collect(),
    )
}

/// Evaluate `expr` in a context where the bare identifier `value` resolves to
/// `current`. Implemented by wrapping the note with a `value` property; other
/// lookups still resolve against the original note.
fn eval_with_value(expr: &Expr, current: &Value, ctx: &EvalCtx) -> Value {
    let mut note = ctx.note.clone();
    note.properties.insert("value".to_string(), current.clone());
    let sub = EvalCtx::new(&note, ctx.corpus, ctx.formulas).with_this(ctx.this);
    sub.eval(expr)
}

// --- helpers ------------------------------------------------------------------

fn title_case(s: &str) -> String {
    s.split_inclusive(char::is_whitespace)
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect()
}

fn slice_chars(s: &str, start: i64, end: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let (a, b) = slice_bounds(chars.len(), start, end);
    chars[a..b].iter().collect()
}

fn slice_vec(items: &[Value], start: i64, end: Option<i64>) -> Vec<Value> {
    let (a, b) = slice_bounds(items.len(), start, end);
    items[a..b].to_vec()
}

/// Normalize JS-style slice bounds (negative from end, clamped) to `[a, b)`.
fn slice_bounds(len: usize, start: i64, end: Option<i64>) -> (usize, usize) {
    let len_i = len as i64;
    let norm = |x: i64| -> i64 {
        let x = if x < 0 { len_i + x } else { x };
        x.clamp(0, len_i)
    };
    let a = norm(start);
    let b = end.map(norm).unwrap_or(len_i);
    (a as usize, b.max(a) as usize)
}

/// Parse an Obsidian duration string like `1d`, `2h`, `30m`, `1w`, `1y`, `3M`,
/// `15s`, or a compound `1d 2h`. Returns total milliseconds.
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    let mut num = String::new();
    let mut matched = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || c == '-' || c == '+' {
            num.push(c);
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_alphabetic() {
            // Read the (possibly multi-char) unit.
            let ustart = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let unit: String = chars[ustart..i].iter().collect();
            let n: i64 = num.parse().ok()?;
            num.clear();
            let ms = unit_to_millis(&unit, n)?;
            total += ms;
            matched = true;
        } else {
            return None;
        }
    }
    if matched && num.is_empty() {
        Some(total)
    } else {
        None
    }
}

fn unit_to_millis(unit: &str, n: i64) -> Option<i64> {
    // Match Obsidian's unit set. Month/year use nominal 30/365-day lengths.
    let per = match unit {
        "ms" => 1,
        "s" | "sec" | "secs" | "second" | "seconds" => 1000,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000,
        "d" | "day" | "days" => 86_400_000,
        "w" | "week" | "weeks" => 604_800_000,
        "M" | "month" | "months" => 2_592_000_000, // 30 days
        "y" | "yr" | "yrs" | "year" | "years" => 31_536_000_000, // 365 days
        _ => return None,
    };
    Some(per * n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EvalCtx;
    use crate::note::Corpus;
    use crate::parser::parse;
    use std::collections::BTreeMap;

    fn eval_str(src: &str) -> Value {
        let note = Note::default();
        let corpus = Corpus::new(vec![note.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(&note, &corpus, &formulas);
        ctx.eval(&parse(src).unwrap())
    }

    #[test]
    fn string_methods() {
        assert!(matches!(
            eval_str(r#""hello".contains("ell")"#),
            Value::Bool(true)
        ));
        assert!(matches!(
            eval_str(r#""hello".startsWith("he")"#),
            Value::Bool(true)
        ));
        assert!(
            matches!(eval_str(r#""hello world".title()"#), Value::Str(s) if s == "Hello World")
        );
        assert!(matches!(eval_str(r#""HELLO".lower()"#), Value::Str(s) if s == "hello"));
        assert!(matches!(eval_str(r#""  hi  ".trim()"#), Value::Str(s) if s == "hi"));
        assert!(matches!(eval_str(r#""hello".slice(1, 4)"#), Value::Str(s) if s == "ell"));
        assert!(matches!(eval_str(r#""123".repeat(2)"#), Value::Str(s) if s == "123123"));
    }

    #[test]
    fn string_split() {
        match eval_str(r#""a,b,c,d".split(",", 3)"#) {
            Value::List(items) => assert_eq!(items.len(), 3),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn number_methods() {
        assert!(matches!(eval_str("(1.23456).toFixed(2)"), Value::Str(s) if s == "1.23"));
        assert!(matches!(eval_str("(2.5).round()"), Value::Number(n) if n == 3.0));
        assert!(matches!(eval_str("(-5).abs()"), Value::Number(n) if n == 5.0));
        assert!(matches!(eval_str("(2.1).ceil()"), Value::Number(n) if n == 3.0));
        assert!(matches!(eval_str("(2.9).floor()"), Value::Number(n) if n == 2.0));
        assert!(matches!(eval_str("(1.23456).round(2)"), Value::Number(n) if n == 1.23));
    }

    #[test]
    fn list_methods() {
        assert!(matches!(
            eval_str("[1, 2, 3].contains(2)"),
            Value::Bool(true)
        ));
        assert!(matches!(eval_str("[1, 2, 3].join(\",\")"), Value::Str(s) if s == "1,2,3"));
        assert!(matches!(eval_str("[3, 1, 2].sort()[0]"), Value::Number(n) if n == 1.0));
        assert!(matches!(eval_str("[1, 2, 2, 3].unique().length"), Value::Number(n) if n == 3.0));
        assert!(matches!(eval_str("[1, [2, 3]].flat().length"), Value::Number(n) if n == 3.0));
    }

    #[test]
    fn list_map_and_filter() {
        assert!(matches!(eval_str("[1, 2, 3, 4].map(value + 1)[0]"), Value::Number(n) if n == 2.0));
        assert!(
            matches!(eval_str("[1, 2, 3, 4].filter(value > 2).length"), Value::Number(n) if n == 2.0)
        );
    }

    #[test]
    fn if_function() {
        assert!(matches!(eval_str(r#"if(true, "y", "n")"#), Value::Str(s) if s == "y"));
        assert!(matches!(eval_str(r#"if(false, "y", "n")"#), Value::Str(s) if s == "n"));
        assert!(matches!(eval_str(r#"if(0, "y")"#), Value::Null));
    }

    #[test]
    fn link_and_list_functions() {
        assert!(matches!(eval_str(r#"link("Books").path"#), Value::Str(s) if s == "Books"));
        assert!(matches!(eval_str("list(5).length"), Value::Number(n) if n == 1.0));
        assert!(matches!(eval_str("number(\"3.4\")"), Value::Number(n) if (n - 3.4).abs() < 1e-9));
        assert!(matches!(eval_str("min(3, 1, 2)"), Value::Number(n) if n == 1.0));
        assert!(matches!(eval_str("max(3, 1, 2)"), Value::Number(n) if n == 3.0));
    }

    #[test]
    fn date_format_method() {
        let d = crate::note::parse_date("2024-01-02").unwrap();
        let note = {
            let mut n = Note::default();
            n.properties.insert("d".into(), Value::Date(d));
            n
        };
        let corpus = Corpus::new(vec![note.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(&note, &corpus, &formulas);
        assert!(
            matches!(ctx.eval(&parse(r#"d.format("YYYY/MM/DD")"#).unwrap()), Value::Str(s) if s == "2024/01/02")
        );
    }

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("1d"), Some(86_400_000));
        assert_eq!(parse_duration("2h"), Some(7_200_000));
        assert_eq!(parse_duration("1d 2h"), Some(86_400_000 + 7_200_000));
        assert_eq!(parse_duration("30m"), Some(1_800_000));
        assert_eq!(parse_duration("1w"), Some(604_800_000));
        assert_eq!(parse_duration("xyz"), None);
    }

    #[test]
    fn any_methods() {
        assert!(matches!(eval_str(r#""".isEmpty()"#), Value::Bool(true)));
        assert!(matches!(eval_str("[].isEmpty()"), Value::Bool(true)));
        assert!(matches!(eval_str("(123).toString()"), Value::Str(s) if s == "123"));
        assert!(matches!(
            eval_str(r#""x".isType("string")"#),
            Value::Bool(true)
        ));
    }

    #[test]
    fn unknown_method_is_null() {
        assert!(matches!(eval_str(r#""x".bogusMethod()"#), Value::Null));
    }
}
