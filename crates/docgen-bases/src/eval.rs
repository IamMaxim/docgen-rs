//! The tree-walking evaluator. Turns an [`Expr`] into a [`Value`] against an
//! [`EvalCtx`] (the current note, the corpus, and the base's formulas). It never
//! panics: unknown identifiers, type mismatches, and out-of-range indices all
//! resolve to [`Value::Null`], matching Obsidian's forgiving evaluation.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::functions;
use crate::note::{Corpus, Note};
use crate::value::{BaseDate, Value};

/// Everything an expression needs to evaluate: the note it's evaluated against,
/// the whole corpus (for backlinks/link resolution), the base's formula
/// definitions, and an optional `this` context note (the note a base is embedded
/// in). Formula evaluation is memoized per note with a cycle guard.
pub struct EvalCtx<'a> {
    pub note: &'a Note,
    pub corpus: &'a Corpus,
    pub this: Option<&'a Note>,
    /// Formula name → parsed expression (shared across the whole base).
    pub formulas: &'a BTreeMap<String, Expr>,
    /// Formula names currently being evaluated (cycle guard).
    active: RefCell<Vec<String>>,
    /// Memoized formula results for this note.
    cache: RefCell<BTreeMap<String, Value>>,
    /// Current evaluation recursion depth (guards against a stack overflow from a
    /// deeply nested AST — e.g. a long member chain the parser built iteratively).
    depth: Cell<usize>,
}

/// Maximum evaluation recursion depth. A deeper AST resolves to `Null` rather than
/// overflowing the stack. Matches the parser's nesting guard in spirit; the
/// parser's node budget keeps ASTs well under this in practice.
pub const MAX_EVAL_DEPTH: usize = 512;

impl<'a> EvalCtx<'a> {
    pub fn new(note: &'a Note, corpus: &'a Corpus, formulas: &'a BTreeMap<String, Expr>) -> Self {
        Self {
            note,
            corpus,
            this: None,
            formulas,
            active: RefCell::new(Vec::new()),
            cache: RefCell::new(BTreeMap::new()),
            depth: Cell::new(0),
        }
    }

    pub fn with_this(mut self, this: Option<&'a Note>) -> Self {
        self.this = this;
        self
    }

    /// Evaluate a named formula against this context's note, memoized, with a
    /// cycle guard (a self-referential formula yields `Null`).
    pub fn eval_formula(&self, name: &str) -> Value {
        if let Some(v) = self.cache.borrow().get(name) {
            return v.clone();
        }
        if self.active.borrow().iter().any(|n| n == name) {
            return Value::Null; // cycle
        }
        let Some(expr) = self.formulas.get(name) else {
            return Value::Null;
        };
        self.active.borrow_mut().push(name.to_string());
        let v = self.eval(expr);
        self.active.borrow_mut().pop();
        self.cache.borrow_mut().insert(name.to_string(), v.clone());
        v
    }

    /// Evaluate an expression to a value. Depth-guarded: a pathologically deep AST
    /// resolves to `Null` past [`MAX_EVAL_DEPTH`] rather than overflowing the stack.
    pub fn eval(&self, expr: &Expr) -> Value {
        let d = self.depth.get();
        if d >= MAX_EVAL_DEPTH {
            return Value::Null;
        }
        self.depth.set(d + 1);
        let v = self.eval_inner(expr);
        self.depth.set(d);
        v
    }

    fn eval_inner(&self, expr: &Expr) -> Value {
        match expr {
            Expr::Null => Value::Null,
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Number(n) => Value::Number(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Regex(p, f) => Value::Str(format!("/{p}/{f}")), // regex used only via .matches
            Expr::Ident(name) => self.ident(name),
            Expr::Member(obj, name) => self.member(obj, name),
            Expr::Index(obj, idx) => self.index(obj, idx),
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Unary(op, e) => self.unary(*op, e),
            Expr::Binary(op, l, r) => self.binary(*op, l, r),
        }
    }

    /// A bare identifier is a note property (the namespace markers `file`/`note`/
    /// `formula`/`this` only appear as the object of a member access, handled in
    /// [`member`]).
    fn ident(&self, name: &str) -> Value {
        match name {
            // A lone namespace word has no value on its own.
            "file" | "note" | "formula" => Value::Null,
            "this" => self
                .this
                .map(|n| Value::Object(n.properties.clone()))
                .unwrap_or(Value::Null),
            _ => self.note.note_property(name),
        }
    }

    fn member(&self, obj: &Expr, name: &str) -> Value {
        // Namespace member accesses resolve against metadata, not a value.
        if let Expr::Ident(ns) = obj {
            match ns.as_str() {
                "file" => return self.note.file_property(name),
                "note" => return self.note.note_property(name),
                "formula" => return self.eval_formula(name),
                "this" => {
                    let this = self.this.unwrap_or(self.note);
                    // `this.file` yields the file namespace; handled when the next
                    // member is accessed. Here `this.<prop>` is a note property.
                    if name == "file" {
                        // Represent `this.file` as the note's file properties object;
                        // `this.file.name` then indexes it below in `index`/member.
                        return Value::Object(file_object(this));
                    }
                    return this.note_property(name);
                }
                _ => {}
            }
        }
        // `this.file.<field>` — object produced above.
        let recv = self.eval(obj);
        value_member(&recv, name)
    }

    fn index(&self, obj: &Expr, idx: &Expr) -> Value {
        let recv = self.eval(obj);
        let key = self.eval(idx);
        match (&recv, &key) {
            (Value::List(items), Value::Number(n)) => match int_index(*n, items.len()) {
                Some(i) => items[i].clone(),
                None => Value::Null,
            },
            (Value::Object(map), Value::Str(k)) => map.get(k).cloned().unwrap_or(Value::Null),
            (Value::Str(s), Value::Number(n)) => {
                let chars: Vec<char> = s.chars().collect();
                match int_index(*n, chars.len()) {
                    Some(i) => Value::Str(chars[i].to_string()),
                    None => Value::Null,
                }
            }
            _ => Value::Null,
        }
    }

    fn call(&self, callee: &Expr, args: &[Expr]) -> Value {
        match callee {
            // Global function call: `link(...)`, `date(...)`, `if(...)`, `[a,b]`.
            Expr::Ident(name) => functions::global_call(name, args, self),
            // Method call. Namespace-object methods (`file.hasTag(...)`) dispatch
            // against metadata; otherwise evaluate the receiver and dispatch.
            Expr::Member(obj, method) => {
                if let Expr::Ident(ns) = &**obj {
                    match ns.as_str() {
                        "file" => return functions::file_method(self.note, method, args, self),
                        "this" => {
                            let this = self.this.unwrap_or(self.note);
                            return functions::file_method(this, method, args, self);
                        }
                        _ => {}
                    }
                }
                // `this.file.hasLink(...)` — obj is `this.file` (a member).
                if let Expr::Member(inner, f) = &**obj {
                    if matches!(&**inner, Expr::Ident(n) if n == "this") && f == "file" {
                        let this = self.this.unwrap_or(self.note);
                        return functions::file_method(this, method, args, self);
                    }
                }
                let recv = self.eval(obj);
                functions::method_call(&recv, method, args, self)
            }
            _ => Value::Null,
        }
    }

    fn unary(&self, op: UnaryOp, e: &Expr) -> Value {
        let v = self.eval(e);
        match op {
            UnaryOp::Not => Value::Bool(!v.is_truthy()),
            UnaryOp::Neg => v
                .as_number()
                .map(|n| Value::Number(-n))
                .unwrap_or(Value::Null),
        }
    }

    fn binary(&self, op: BinaryOp, l: &Expr, r: &Expr) -> Value {
        // Short-circuit boolean operators.
        match op {
            BinaryOp::And => {
                let lv = self.eval(l);
                return if !lv.is_truthy() {
                    Value::Bool(false)
                } else {
                    Value::Bool(self.eval(r).is_truthy())
                };
            }
            BinaryOp::Or => {
                let lv = self.eval(l);
                return if lv.is_truthy() {
                    Value::Bool(true)
                } else {
                    Value::Bool(self.eval(r).is_truthy())
                };
            }
            _ => {}
        }
        let lv = self.eval(l);
        let rv = self.eval(r);
        match op {
            BinaryOp::Eq => Value::Bool(lv.loose_eq(&rv)),
            BinaryOp::NotEq => Value::Bool(!lv.loose_eq(&rv)),
            BinaryOp::Lt => Value::Bool(lv.loose_cmp(&rv).is_lt()),
            BinaryOp::Gt => Value::Bool(lv.loose_cmp(&rv).is_gt()),
            BinaryOp::LtEq => Value::Bool(lv.loose_cmp(&rv).is_le()),
            BinaryOp::GtEq => Value::Bool(lv.loose_cmp(&rv).is_ge()),
            BinaryOp::Add => add(&lv, &rv),
            BinaryOp::Sub => sub(&lv, &rv),
            BinaryOp::Mul => arith(&lv, &rv, |a, b| a * b),
            BinaryOp::Div => arith(&lv, &rv, |a, b| a / b),
            BinaryOp::Mod => arith(&lv, &rv, |a, b| a % b),
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }
}

/// Build the `file.*` property object (for `this.file.<field>`).
fn file_object(note: &Note) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    for f in [
        "name", "basename", "path", "folder", "ext", "size", "ctime", "mtime", "tags", "links",
    ] {
        m.insert(f.to_string(), note.file_property(f));
    }
    m
}

/// Resolve a numeric index into a collection of length `len`, applying Obsidian's
/// rules: the index must be a finite integer (a fractional or NaN index is
/// invalid → `None`); a negative index counts from the end; out-of-range → `None`.
fn int_index(n: f64, len: usize) -> Option<usize> {
    if !n.is_finite() || n.fract() != 0.0 {
        return None;
    }
    let i = n as i64;
    let len_i = len as i64;
    let i = if i < 0 { len_i + i } else { i };
    if i >= 0 && i < len_i {
        Some(i as usize)
    } else {
        None
    }
}

/// Member access on a *value* (not a namespace): list/string `.length`, date
/// fields, object keys, link `.path`/`.display`.
pub fn value_member(recv: &Value, name: &str) -> Value {
    match recv {
        Value::List(items) => match name {
            "length" => Value::Number(items.len() as f64),
            _ => Value::Null,
        },
        Value::Str(s) => match name {
            "length" => Value::Number(s.chars().count() as f64),
            _ => Value::Null,
        },
        Value::Object(map) => map.get(name).cloned().unwrap_or(Value::Null),
        Value::Date(d) => date_field(d, name),
        Value::Link(l) => match name {
            "path" => Value::Str(l.path.clone()),
            "display" => l.display.clone().map(Value::Str).unwrap_or(Value::Null),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

fn date_field(d: &BaseDate, name: &str) -> Value {
    match name {
        "year" => Value::Number(d.year as f64),
        "month" => Value::Number(d.month as f64),
        "day" => Value::Number(d.day as f64),
        "hour" => Value::Number(d.hour as f64),
        "minute" => Value::Number(d.minute as f64),
        "second" => Value::Number(d.second as f64),
        "millisecond" => Value::Number(d.millisecond as f64),
        _ => Value::Null,
    }
}

/// `+`: numeric when both coerce to numbers, date±duration handled in [`add`],
/// otherwise string concatenation (matching Obsidian's `"a" + b`).
fn add(l: &Value, r: &Value) -> Value {
    // date + duration → date.
    if let (Value::Date(d), Value::Duration(ms)) = (l, r) {
        return Value::Date(add_duration(d, *ms));
    }
    if let (Value::Duration(ms), Value::Date(d)) = (l, r) {
        return Value::Date(add_duration(d, *ms));
    }
    if let (Value::Duration(a), Value::Duration(b)) = (l, r) {
        return Value::Duration(a + b);
    }
    // If either side is a string, concatenate string renderings.
    if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) {
        return Value::Str(format!("{}{}", l.as_str_coerced(), r.as_str_coerced()));
    }
    match (l.as_number(), r.as_number()) {
        (Some(a), Some(b)) => Value::Number(a + b),
        _ => Value::Str(format!("{}{}", l.as_str_coerced(), r.as_str_coerced())),
    }
}

/// `-`: date−duration → date, date−date → duration (ms), else numeric.
fn sub(l: &Value, r: &Value) -> Value {
    if let (Value::Date(d), Value::Duration(ms)) = (l, r) {
        return Value::Date(add_duration(d, -*ms));
    }
    if let (Value::Date(a), Value::Date(b)) = (l, r) {
        return Value::Duration(a.epoch_millis() - b.epoch_millis());
    }
    if let (Value::Duration(a), Value::Duration(b)) = (l, r) {
        return Value::Duration(a - b);
    }
    arith(l, r, |a, b| a - b)
}

fn arith(l: &Value, r: &Value, f: impl Fn(f64, f64) -> f64) -> Value {
    match (l.as_number(), r.as_number()) {
        (Some(a), Some(b)) => Value::Number(f(a, b)),
        _ => Value::Null,
    }
}

/// Add `ms` milliseconds to a date by converting to epoch-millis and back.
pub fn add_duration(d: &BaseDate, ms: i64) -> BaseDate {
    let total = d.epoch_millis() + ms;
    date_from_epoch_millis(total, d.has_time || ms % 86_400_000 != 0)
}

/// Convert epoch-millis back to a broken-down [`BaseDate`] (inverse of
/// `epoch_millis`, via the civil-from-days algorithm).
pub fn date_from_epoch_millis(ms: i64, has_time: bool) -> BaseDate {
    let mut days = ms.div_euclid(86_400_000);
    let mut rem = ms.rem_euclid(86_400_000);
    let millisecond = (rem % 1000) as u32;
    rem /= 1000;
    let second = (rem % 60) as u32;
    rem /= 60;
    let minute = (rem % 60) as u32;
    rem /= 60;
    let hour = (rem % 24) as u32;
    // civil_from_days (Howard Hinnant).
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    BaseDate {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
        has_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Corpus;
    use crate::parser::parse;
    use crate::value::BaseLink;

    fn eval_str(src: &str, note: &Note) -> Value {
        let corpus = Corpus::new(vec![note.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(note, &corpus, &formulas);
        ctx.eval(&parse(src).unwrap())
    }

    fn note_with(props: &[(&str, Value)]) -> Note {
        let mut n = Note::default();
        for (k, v) in props {
            n.properties.insert(k.to_string(), v.clone());
        }
        n
    }

    #[test]
    fn arithmetic_and_precedence() {
        let n = Note::default();
        assert!(matches!(eval_str("1 + 2 * 3", &n), Value::Number(x) if x == 7.0));
        assert!(matches!(eval_str("(1 + 2) * 3", &n), Value::Number(x) if x == 9.0));
        assert!(matches!(eval_str("10 % 3", &n), Value::Number(x) if x == 1.0));
    }

    #[test]
    fn comparisons_and_logic() {
        let n = note_with(&[("age", Value::Number(30.0))]);
        assert!(matches!(eval_str("age > 18", &n), Value::Bool(true)));
        assert!(matches!(
            eval_str("age > 18 && age < 40", &n),
            Value::Bool(true)
        ));
        assert!(matches!(
            eval_str("age < 18 || age > 25", &n),
            Value::Bool(true)
        ));
        assert!(matches!(eval_str("!(age > 18)", &n), Value::Bool(false)));
    }

    #[test]
    fn bare_and_note_property() {
        let n = note_with(&[("type", Value::Str("home".into()))]);
        assert!(matches!(
            eval_str(r#"type == "home""#, &n),
            Value::Bool(true)
        ));
        assert!(matches!(
            eval_str(r#"note.type == "home""#, &n),
            Value::Bool(true)
        ));
    }

    #[test]
    fn file_properties() {
        let mut n = Note::default();
        n.name = "Intro.md".into();
        n.ext = "md".into();
        n.folder = "guide".into();
        assert!(matches!(
            eval_str(r#"file.ext == "md""#, &n),
            Value::Bool(true)
        ));
        assert!(matches!(
            eval_str(r#"file.name == "Intro.md""#, &n),
            Value::Bool(true)
        ));
    }

    #[test]
    fn string_concat_and_number_coercion() {
        let n = note_with(&[("price", Value::Number(3.0))]);
        assert!(matches!(eval_str(r#""$" + price"#, &n), Value::Str(s) if s == "$3"));
    }

    #[test]
    fn list_index_and_length() {
        let n = note_with(&[(
            "cats",
            Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]),
        )]);
        assert!(matches!(eval_str("cats[0]", &n), Value::Str(s) if s == "a"));
        assert!(matches!(eval_str("cats[-1]", &n), Value::Str(s) if s == "b"));
        assert!(matches!(eval_str("cats.length", &n), Value::Number(x) if x == 2.0));
    }

    #[test]
    fn string_negative_index_counts_from_end() {
        let n = note_with(&[("title", Value::Str("abc".into()))]);
        assert!(matches!(eval_str("title[-1]", &n), Value::Str(s) if s == "c"));
        assert!(matches!(eval_str("title[0]", &n), Value::Str(s) if s == "a"));
        assert!(matches!(eval_str("title[5]", &n), Value::Null));
    }

    #[test]
    fn fractional_and_nan_index_are_null() {
        let n = note_with(&[(
            "cats",
            Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]),
        )]);
        assert!(matches!(eval_str("cats[1.9]", &n), Value::Null));
        // 0/0 → NaN index → Null (not element 0).
        assert!(matches!(eval_str("cats[0 / 0]", &n), Value::Null));
    }

    #[test]
    fn deep_member_chain_eval_does_not_overflow() {
        // A member chain under the parser's node budget still parses, but evaluating
        // it recurses deeper than MAX_EVAL_DEPTH — the eval guard must return Null
        // rather than overflow the stack.
        let n = Note::default();
        let corpus = Corpus::new(vec![n.clone()]);
        let formulas = BTreeMap::new();
        let ctx = EvalCtx::new(&n, &corpus, &formulas);
        let expr = format!("a{}", ".a".repeat(2000)); // 2001 nodes < 4096 budget
        let parsed = crate::parser::parse(&expr).expect("under node budget → parses");
        assert!(matches!(ctx.eval(&parsed), Value::Null));
    }

    #[test]
    fn date_field_access() {
        let n = note_with(&[(
            "created",
            Value::Date(crate::note::parse_date("2024-05-06").unwrap()),
        )]);
        assert!(matches!(eval_str("created.year", &n), Value::Number(x) if x == 2024.0));
        assert!(matches!(eval_str("created.month", &n), Value::Number(x) if x == 5.0));
    }

    #[test]
    fn date_minus_date_is_duration() {
        let n = note_with(&[
            (
                "a",
                Value::Date(crate::note::parse_date("2024-01-02").unwrap()),
            ),
            (
                "b",
                Value::Date(crate::note::parse_date("2024-01-01").unwrap()),
            ),
        ]);
        // One day in ms.
        assert!(matches!(eval_str("a - b", &n), Value::Duration(ms) if ms == 86_400_000));
    }

    #[test]
    fn unknown_symbols_are_null_not_panic() {
        let n = Note::default();
        assert!(matches!(eval_str("nonexistent", &n), Value::Null));
        assert!(matches!(eval_str("nope.deep.field", &n), Value::Null));
        assert!(matches!(eval_str("missing[3]", &n), Value::Null));
    }

    #[test]
    fn formula_evaluation_with_cache() {
        let n = note_with(&[("price", Value::Number(10.0)), ("age", Value::Number(2.0))]);
        let corpus = Corpus::new(vec![n.clone()]);
        let mut formulas = BTreeMap::new();
        formulas.insert("ppu".to_string(), parse("price / age").unwrap());
        let ctx = EvalCtx::new(&n, &corpus, &formulas);
        assert!(matches!(ctx.eval(&parse("formula.ppu").unwrap()), Value::Number(x) if x == 5.0));
    }

    #[test]
    fn formula_cycle_yields_null() {
        let n = Note::default();
        let corpus = Corpus::new(vec![n.clone()]);
        let mut formulas = BTreeMap::new();
        formulas.insert("a".to_string(), parse("formula.b").unwrap());
        formulas.insert("b".to_string(), parse("formula.a").unwrap());
        let ctx = EvalCtx::new(&n, &corpus, &formulas);
        assert!(matches!(
            ctx.eval(&parse("formula.a").unwrap()),
            Value::Null
        ));
    }

    #[test]
    fn link_contains_via_method() {
        let n = note_with(&[(
            "categories",
            Value::List(vec![Value::Link(BaseLink::new("Categories/Books"))]),
        )]);
        assert!(matches!(
            eval_str(
                r#"categories.contains(link("Categories/Books", "Books"))"#,
                &n
            ),
            Value::Bool(true)
        ));
        assert!(matches!(
            eval_str(r#"categories.contains(link("Books"))"#, &n),
            Value::Bool(true)
        ));
    }

    #[test]
    fn round_trip_epoch_conversion() {
        let d = crate::note::parse_date("2024-05-06 07:08:09").unwrap();
        let back = date_from_epoch_millis(d.epoch_millis(), true);
        assert_eq!((back.year, back.month, back.day), (2024, 5, 6));
        assert_eq!((back.hour, back.minute, back.second), (7, 8, 9));
    }
}
