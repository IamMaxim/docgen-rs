//! Version-aware column sorting.
//!
//! A `note.version` column of `1.0.23 / 1.2.12 / 1.19.20` sorts wrong under the
//! plain string ordering `loose_cmp` uses (`1.19.20` lands before `1.2.12`,
//! because `'1' < '2'` at the third character). This module recognises version
//! columns and orders them numerically instead.
//!
//! ## Sort keys, not a comparator
//!
//! The entry point is [`semver_key`], which maps a version string to an opaque
//! ASCII key whose *lexicographic* order is its version order. Both the static
//! renderer and the interactive island sort by comparing these keys, so the
//! version rules live here once and the browser only ever does `a < b` on a
//! string the renderer already computed. That is what keeps client-side sorting
//! in step with the server — the island cannot drift because it never parses a
//! version at all (see `interactive.rs`).
//!
//! Keys are pure ASCII, so Rust's byte ordering and JavaScript's UTF-16 code
//! unit ordering agree exactly. Non-ASCII input is rejected rather than encoded,
//! which keeps that guarantee true by construction.
//!
//! ## What parses (lenient)
//!
//! `1.2`, `v1.2.3`, `1.2.3-rc.1`, `1.2.3+build.5` — a leading `v` is ignored,
//! omitted parts default to `0`, build metadata is discarded (semver spec: it
//! carries no precedence), and a pre-release sorts *before* its release.
//! Two or three numeric core parts are required, so a column of bare integers
//! stays on its existing numeric/string path rather than being reinterpreted.

use crate::value::Value;

/// Width that every numeric run is zero-padded to. `u64::MAX` is 20 digits, so
/// this makes fixed-width fields whose lexicographic order matches their
/// numeric order.
const PAD: usize = 20;

/// Sorts after any pre-release suffix. Must exceed every byte [`encode_pre`] can
/// emit; `~` (0x7E) is above all ASCII alphanumerics.
const RELEASE: char = '~';
/// Introduces a pre-release. Must be below [`RELEASE`] and below every byte
/// [`encode_pre`] emits, so `1.2.3-rc1` orders before `1.2.3`.
const PRERELEASE: char = '!';

/// Maps a version string to a key whose lexicographic order is version order,
/// or `None` if it does not parse as a version.
///
/// The key is opaque: only its ordering relative to other keys is meaningful.
pub fn semver_key(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || !s.is_ascii() {
        return None;
    }
    // Strip a leading `v`/`V` (`v1.2.3`), which is conventional but not semver.
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);

    // Build metadata carries no precedence — discard it before anything else.
    let s = s.split('+').next().unwrap_or(s);

    // Split core from pre-release at the FIRST `-`: `1.2.3-rc-1` is core `1.2.3`
    // with pre-release `rc-1`.
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };

    let mut key = String::new();
    let mut parts = 0usize;
    for part in core.split('.') {
        parts += 1;
        if parts > 3 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // Reject values too large to compare numerically rather than silently
        // truncating them into the wrong order.
        let n: u64 = part.parse().ok()?;
        if parts > 1 {
            key.push('.');
        }
        key.push_str(&format!("{n:0PAD$}"));
    }
    // Require a dotted version, so bare integers are not treated as versions.
    if parts < 2 {
        return None;
    }
    // Pad omitted parts so `1.2` and `1.2.0` produce identical keys.
    for _ in parts..3 {
        key.push('.');
        key.push_str(&format!("{:0PAD$}", 0));
    }

    match pre {
        None => key.push(RELEASE),
        Some(p) => {
            key.push(PRERELEASE);
            key.push_str(&encode_pre(p)?);
        }
    }
    Some(key)
}

/// Encodes a pre-release suffix (the part after `-`) so that lexicographic
/// order matches semver precedence: identifiers compare left to right, numeric
/// ones rank below alphanumeric ones and compare numerically, and a shorter set
/// of identifiers ranks below a longer one that shares its prefix.
fn encode_pre(pre: &str) -> Option<String> {
    let mut out = String::new();
    for (i, id) in pre.split('.').enumerate() {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return None;
        }
        if i > 0 {
            // Below every byte an identifier can start with, so a prefix-sharing
            // shorter identifier list sorts first.
            out.push('.');
        }
        if id.bytes().all(|b| b.is_ascii_digit()) {
            let n: u64 = id.parse().ok()?;
            // `0` tags numeric identifiers, `1` alphanumeric — numeric first.
            out.push('0');
            out.push_str(&format!("{n:0PAD$}"));
        } else {
            out.push('1');
            out.push_str(id);
        }
    }
    Some(out)
}

/// Ranks after every version key, which always begins with a padded digit.
/// Unparseable values sort before empty ones, mirroring how `loose_cmp` puts
/// `Null` last.
const JUNK_KEY: &str = "~1";
const EMPTY_KEY: &str = "~2";

/// Total-order key for one cell of a version column — defined for *every*
/// value, including empty and unparseable ones.
///
/// Every cell in a version column gets one of these in the payload, so the
/// island can order the column with a plain `<` on strings and cannot disagree
/// with the renderer. A per-cell key is what makes that possible: were the
/// island left to infer "is this a version column?" itself, it would have to
/// re-implement the parser, and pairs the renderer calls equal (two
/// unparseable values, say) would silently order differently in the browser.
pub fn column_sort_key(v: &Value) -> String {
    if v.is_empty() {
        return EMPTY_KEY.to_string();
    }
    value_semver_key(v).unwrap_or_else(|| JUNK_KEY.to_string())
}

/// The sort key for a value, if it is a version.
///
/// Only [`Value::Str`] is considered. A YAML `version: 1.5` parses as a number,
/// and numbers already sort numerically — reinterpreting them here would change
/// `1.5 > 1.10` into `1.5 < 1.10` on columns that never asked for versions.
pub fn value_semver_key(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => semver_key(s),
        _ => None,
    }
}

/// Whether a column should sort as versions: every non-empty value parses.
///
/// An all-empty column returns `false` — there is nothing to order, and the
/// existing path already handles it.
pub fn column_is_semver<'a>(values: impl Iterator<Item = &'a Value>) -> bool {
    let mut saw_one = false;
    for v in values {
        if v.is_empty() {
            continue;
        }
        if value_semver_key(v).is_none() {
            return false;
        }
        saw_one = true;
    }
    saw_one
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert `a < b` by key, and that the reverse holds — a key ordering that
    /// is not antisymmetric would make `sort_by` produce garbage.
    fn lt(a: &str, b: &str) {
        let (ka, kb) = (semver_key(a).unwrap(), semver_key(b).unwrap());
        assert!(ka < kb, "expected {a} < {b} (keys {ka:?} vs {kb:?})");
        assert!(kb > ka, "expected {b} > {a} (asymmetry)");
    }

    fn eq(a: &str, b: &str) {
        assert_eq!(semver_key(a).unwrap(), semver_key(b).unwrap(), "{a} == {b}");
    }

    #[test]
    fn orders_core_numerically_not_lexically() {
        // The bug that motivates the module: lexically "1.19.20" < "1.2.12".
        lt("1.0.23", "1.2.12");
        lt("1.2.12", "1.19.20");
        lt("1.9.0", "1.10.0");
        lt("2.0.0", "10.0.0");
    }

    #[test]
    fn lenient_forms_normalise() {
        eq("v1.2.3", "1.2.3");
        eq("V1.2.3", "1.2.3");
        eq("1.2", "1.2.0");
        eq("  1.2.3  ", "1.2.3");
        // Build metadata carries no precedence.
        eq("1.2.3+build.5", "1.2.3");
        eq("1.2.3+exp.sha.5114f85", "1.2.3");
    }

    #[test]
    fn prerelease_sorts_before_its_release() {
        lt("1.0.0-rc1", "1.0.0");
        lt("1.0.0-alpha", "1.0.0-beta");
        // A pre-release of the NEXT version still outranks this release.
        lt("1.0.0", "1.0.1-alpha");
    }

    /// The precedence example from semver.org §11, which exercises every rule:
    /// numeric < alphanumeric, numeric compares numerically, shorter set first.
    #[test]
    fn semver_spec_precedence_example() {
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            lt(pair[0], pair[1]);
        }
    }

    #[test]
    fn numeric_prerelease_ids_compare_numerically() {
        // The same trap as the core, one level down: lexically "11" < "2".
        lt("1.0.0-beta.2", "1.0.0-beta.11");
        lt("1.0.0-1", "1.0.0-2");
        lt("1.0.0-9", "1.0.0-10");
    }

    #[test]
    fn rejects_non_versions() {
        for s in [
            "",
            "   ",
            "abc",
            "1",                       // bare integer: not a dotted version
            "42",                      //
            "1.2.3.4",                 // too many parts
            "1..2",                    // empty part
            "1.x",                     // non-numeric part
            "1.2-",                    // empty pre-release
            "1.2-rc/1",                // illegal pre-release character
            "1.2.3-café",              // non-ASCII
            "99999999999999999999999", // overflows u64
        ] {
            assert!(semver_key(s).is_none(), "expected {s:?} to be rejected");
        }
    }

    /// Keys must stay ASCII: JS compares them with `<` over UTF-16 code units,
    /// which only agrees with Rust's byte order for ASCII.
    #[test]
    fn keys_are_ascii() {
        for s in ["1.2.3", "v1.0.0-rc.1", "1.2", "1.0.0-alpha.beta"] {
            assert!(semver_key(s).unwrap().is_ascii(), "{s} produced non-ASCII");
        }
    }

    #[test]
    fn column_detection_requires_all_non_empty_to_parse() {
        let ver = |s: &str| Value::Str(s.into());
        assert!(column_is_semver([ver("1.2.3"), ver("1.10.0")].iter()));
        // Empties are ignored, not disqualifying.
        assert!(column_is_semver(
            [ver("1.2.3"), Value::Null, ver("2.0.0")].iter()
        ));
        // One stray non-version disqualifies the column.
        assert!(!column_is_semver([ver("1.2.3"), ver("nightly")].iter()));
        // Nothing to order.
        assert!(!column_is_semver([Value::Null].iter()));
        assert!(!column_is_semver([].iter()));
    }

    /// Numbers keep their numeric ordering: `1.5 > 1.10` as decimals, and a
    /// version column must not silently invert that.
    #[test]
    fn numbers_are_not_versions() {
        assert!(value_semver_key(&Value::Number(1.5)).is_none());
        assert!(!column_is_semver(
            [Value::Number(1.5), Value::Number(1.10)].iter()
        ));
    }
}
