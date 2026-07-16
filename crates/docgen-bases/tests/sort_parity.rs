//! Cross-language SORT-PARITY harness (Milestone 7 of the interactive-bases
//! feature).
//!
//! This test proves that the JS island's comparator (`sortIds` / `compareCells`
//! in `crates/docgen-assets/assets/docgen/islands/bases.js`) reproduces the Rust
//! engine's ordering (`Value::loose_cmp` via `render.rs::apply_sort`) on an
//! ADVERSARIAL corpus — and that a future change to EITHER side is caught
//! automatically.
//!
//! ## How the two sides share a checkpoint
//!
//! The renderer assigns each row `id = its index in the post-sort slice`, so the
//! `data-row` ids emitted in document order are simply `0..n` and the payload
//! `rows[]` are in that same Rust-sorted order. This Rust test:
//!   1. builds an adversarial corpus,
//!   2. for several sort configs renders with `interactive: true`,
//!   3. extracts (a) the v1 payload JSON and (b) the `data-row` id sequence,
//!   4. asserts (b) == the payload `rows[].id` order (emitter self-consistency),
//!   5. writes committed fixtures `crates/docgen-assets/js-tests/fixtures/sort-*.json`.
//!
//! The committed fixtures are the oracle the Node parity test
//! (`crates/docgen-assets/js-tests/sort-parity.test.mjs`) replays: it scrambles
//! `payload.rows` to a canonical order, runs the island's `sortIds`, and asserts
//! the result equals `rustOrder`. If the JS comparator ever drifts from Rust, that
//! Node test fails; if the RUST side drifts, THIS test fails (the regenerated
//! payload/order no longer matches the committed fixture).
//!
//! Set `BLESS=1` to (re)write the fixtures. Run once with `BLESS=1` to create
//! them, then without to confirm the guard passes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use docgen_bases::model::{SortKey, View};
use docgen_bases::{render_base, BaseDate, BaseFile, BaseLink, Corpus, Note, RenderOptions, Value};
use serde_json::{json, Value as J};

// ---------------------------------------------------------------------------
// Value constructors
// ---------------------------------------------------------------------------

fn num(n: f64) -> Value {
    Value::Number(n)
}
fn s(t: &str) -> Value {
    Value::Str(t.into())
}
fn boolean(b: bool) -> Value {
    Value::Bool(b)
}
fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|x| Value::Str((*x).into())).collect())
}
/// A date; `has_time` toggles whether the source carried a time component (drives
/// display, NOT the epoch used for ordering).
fn date(y: i64, mo: u32, dy: u32, h: u32, mi: u32, sec: u32, has_time: bool) -> Value {
    Value::Date(BaseDate {
        year: y,
        month: mo,
        day: dy,
        hour: h,
        minute: mi,
        second: sec,
        millisecond: 0,
        has_time,
    })
}
fn link(path: &str) -> Value {
    Value::Link(BaseLink::new(path))
}
fn link_disp(path: &str, disp: &str) -> Value {
    Value::Link(BaseLink::with_display(path, disp))
}

// ---------------------------------------------------------------------------
// Adversarial corpus
// ---------------------------------------------------------------------------

/// The columns every view renders, in a fixed order. `file.name` is a stable
/// unique identifier (so a human can eyeball which note landed where); the rest
/// are the adversarial data columns. Every sort key below refers to one of these.
const COLUMNS: &[&str] = &[
    "file.name",
    "note.num",
    "note.numstr",
    "note.mixed_num",
    "note.date",
    "note.mixed_date",
    "note.flag",
    "note.label",
    "note.owner",
    "note.tags",
];

/// Build a note with a unique `file.name` plus a set of typed properties. Any
/// column absent from `props` is left NULL for that note (exercising nulls /
/// empties in every column).
fn note(name: &str, props: Vec<(&str, Value)>) -> Note {
    let mut n = Note {
        slug: name.to_string(),
        basename: name.to_string(),
        name: format!("{name}.md"),
        path: format!("{name}.md"),
        ext: "md".into(),
        ..Default::default()
    };
    for (k, v) in props {
        n.properties.insert(k.to_string(), v);
    }
    n
}

/// The adversarial corpus. Every column carries a deliberately hostile spread:
/// negative/zero/large numbers, numeric-vs-lexical strings, MIXED-type columns
/// (numbers + numeric-strings + a non-numeric string; dates + strings), equal
/// date instants (`has_time` vs not), case-insensitive strings, links (display vs
/// basename), lists, exact duplicate keys (to exercise the stable id tiebreak),
/// and NULLs sprinkled through every column.
fn corpus() -> Corpus {
    let notes = vec![
        note(
            "note-00",
            vec![
                ("num", num(-5.0)),
                ("numstr", s("10")),
                ("mixed_num", num(3.0)),
                ("date", date(2021, 6, 1, 0, 0, 0, false)),
                ("mixed_date", date(2020, 1, 1, 0, 0, 0, false)),
                ("flag", boolean(true)),
                ("label", s("apple")),
                ("owner", link("People/Alice")),
                ("tags", list(&["api", "db"])),
            ],
        ),
        note(
            "note-01",
            vec![
                ("num", num(0.0)),
                ("numstr", s("2")),
                ("mixed_num", s("10")),
                // Equal instant to note-00 (same epoch) but has_time = true.
                ("date", date(2021, 6, 1, 0, 0, 0, true)),
                ("mixed_date", s("zebra")),
                ("flag", boolean(false)),
                ("label", s("Banana")),
                ("owner", link_disp("People/bob", "Bob")),
                ("tags", list(&["db"])),
            ],
        ),
        note(
            "note-02",
            vec![
                ("num", num(3.0)),
                ("numstr", s("1")),
                ("mixed_num", num(-1.0)),
                ("date", date(2019, 12, 31, 23, 59, 59, true)),
                ("mixed_date", date(2022, 5, 5, 12, 0, 0, true)),
                ("flag", boolean(true)),
                ("label", s("apricot")),
                ("owner", link("carol")),
                ("tags", list(&["api", "ui", "db"])),
            ],
        ),
        note(
            "note-03",
            vec![
                // Exact duplicate sort keys vs note-04 on (flag, num) to force the
                // stable id tiebreak in the multi-key config.
                ("num", num(42.0)),
                ("numstr", s("100")),
                ("mixed_num", s("2")),
                ("date", date(2025, 1, 1, 0, 0, 0, false)),
                ("mixed_date", s("alpha")),
                ("flag", boolean(true)),
                ("label", s("BANANA")),
                ("owner", link_disp("x/y", "alice")),
                ("tags", list(&["ui"])),
            ],
        ),
        note(
            "note-04",
            vec![
                ("num", num(42.0)),
                ("numstr", s("-7")),
                ("mixed_num", s("abc")),
                ("date", date(2018, 3, 15, 8, 30, 0, true)),
                ("mixed_date", date(2020, 1, 1, 0, 0, 0, true)),
                ("flag", boolean(true)),
                ("label", s("cherry")),
                ("owner", link("People/dave")),
                ("tags", list(&["db", "api"])),
            ],
        ),
        note(
            "note-05",
            vec![
                ("num", num(9_000_000_000_000.0)),
                ("numstr", s("30")),
                ("mixed_num", num(0.0)),
                ("date", date(2021, 6, 2, 0, 0, 0, false)),
                ("mixed_date", s("Beta")),
                ("flag", boolean(false)),
                ("label", s("Cherry")),
                ("owner", link_disp("People/erin", "Erin")),
                ("tags", list(&["ui", "api"])),
            ],
        ),
        note(
            "note-06",
            vec![
                ("num", num(1.0)),
                ("numstr", s("3")),
                // mixed_num NULL
                ("date", date(2020, 2, 29, 0, 0, 0, false)),
                ("mixed_date", date(2019, 7, 4, 0, 0, 0, false)),
                ("flag", boolean(false)),
                ("label", s("date")),
                ("owner", link("apple")),
                ("tags", Value::List(vec![])), // empty list → "(empty)" facet, empty display
            ],
        ),
        note(
            "note-07",
            vec![
                ("num", num(2.0)),
                ("numstr", s("2")), // exact dup of note-01 numstr
                ("mixed_num", num(1000.0)),
                // date NULL
                ("mixed_date", date(2022, 5, 5, 12, 0, 0, false)),
                ("flag", boolean(true)),
                ("label", s("apple")), // exact dup of note-00 label
                ("owner", link_disp("People/alice", "alice")),
                ("tags", list(&["zeta"])),
            ],
        ),
        note(
            "note-08",
            vec![
                ("num", num(10.0)),
                // numstr NULL
                ("mixed_num", s("-3")),
                ("date", date(2023, 11, 30, 6, 15, 0, true)),
                ("mixed_date", s("")), // empty string → empty cell
                // flag NULL
                ("label", s("")),                // empty string
                ("owner", link("People/Alice")), // same target as note-00
                ("tags", list(&["api"])),
            ],
        ),
        note(
            "note-09",
            vec![
                ("num", num(0.0)),        // dup of note-01 num
                ("numstr", s("apple")),   // non-numeric string in an otherwise numeric-string col
                ("mixed_num", num(-1.0)), // dup of note-02 mixed_num
                ("date", date(1999, 1, 1, 0, 0, 0, false)),
                ("mixed_date", date(2020, 1, 1, 0, 0, 0, false)), // dup instant of note-00
                ("flag", boolean(false)),
                ("label", s("Éclair")), // non-ASCII to stress lowercase
                ("owner", link_disp("People/zed", "Zed")),
                // tags NULL
            ],
        ),
        note(
            "note-10",
            vec![
                // num NULL
                ("numstr", s("10")),   // dup of note-00 numstr
                ("mixed_num", s("2")), // dup of note-03 mixed_num
                ("date", date(2021, 6, 1, 12, 0, 0, true)),
                // mixed_date NULL
                ("flag", boolean(true)),
                ("label", s("banana")), // case variant of Banana/BANANA
                // owner NULL
                ("tags", list(&["db", "ui", "api"])),
            ],
        ),
        note(
            "note-11",
            vec![
                ("num", num(-5.0)),                         // dup of note-00 num
                ("numstr", s("1")),                         // dup of note-02 numstr
                ("mixed_num", num(3.0)),                    // dup of note-00 mixed_num
                ("date", date(2021, 6, 1, 0, 0, 0, false)), // 3rd equal instant
                ("mixed_date", date(2018, 6, 6, 0, 0, 0, false)),
                ("flag", boolean(true)),
                ("label", s("Apple")), // case variant of apple
                ("owner", link("zulu")),
                ("tags", list(&["api", "db"])), // dup of note-00 tags
            ],
        ),
    ];
    Corpus::new(notes)
}

// ---------------------------------------------------------------------------
// Sort configs to exercise
// ---------------------------------------------------------------------------

/// Each config is a list of `(column-key, descending)` sort keys.
fn configs() -> Vec<Vec<(&'static str, bool)>> {
    vec![
        // 0: single ascending on a pure-number column.
        vec![("note.num", false)],
        // 1: single DESCENDING on the same column — verifies null-FIRST under desc
        //    (apply_sort does `ord.reverse()` over the WHOLE cmp incl. the null branch).
        vec![("note.num", true)],
        // 2: a MIXED-type column (numbers + numeric-strings + a non-numeric string
        //    + nulls) — fires loose_cmp's cross-type branches.
        vec![("note.mixed_num", false)],
        // 3: dates, including equal instants (has_time vs not) — date↔date by epoch.
        vec![("note.date", false)],
        // 4: MIXED dates + strings — proves `epoch` is NOT used cross-type (display).
        vec![("note.mixed_date", false)],
        // 5: case-insensitive string ordering (incl. non-ASCII).
        vec![("note.label", false)],
        // 6: links (display vs basename), case-insensitive by display.
        vec![("note.owner", false)],
        // 7: a list column (sorts by rendered display via the generic branch).
        vec![("note.tags", false)],
        // 8: multi-key with ties — bool primary (many ties) then number, with exact
        //    duplicate (flag, num) rows forcing the final stable id tiebreak.
        vec![("note.flag", false), ("note.num", false)],
    ]
}

// ---------------------------------------------------------------------------
// Rendering + extraction
// ---------------------------------------------------------------------------

fn interactive_opts() -> RenderOptions {
    RenderOptions {
        base: String::new(),
        default_view_name: "Parity".into(),
        interactive: true,
    }
}

fn build_base(keys: &[(&str, bool)]) -> BaseFile {
    let sort: Vec<SortKey> = keys
        .iter()
        .map(|(col, desc)| SortKey::Full {
            property: (*col).to_string(),
            direction: Some(if *desc { "DESC" } else { "ASC" }.to_string()),
        })
        .collect();
    let view = View {
        view_type: "table".into(),
        order: COLUMNS.iter().map(|c| c.to_string()).collect(),
        sort,
        ..Default::default()
    };
    BaseFile {
        views: vec![view],
        ..Default::default()
    }
}

/// Extract the JSON text inside the `docgen-base-data` script element and undo the
/// `</` → `<\/` script-embedding escape so it parses as raw JSON.
fn extract_payload(html: &str) -> J {
    let marker = "class=\"docgen-base-data\">";
    let start = html.find(marker).expect("payload script present") + marker.len();
    let rest = &html[start..];
    let end = rest.find("</script>").expect("payload script closes");
    let raw = rest[..end].replace("<\\/", "</");
    serde_json::from_str(&raw).expect("payload is valid JSON")
}

/// The `data-row` id sequence in document order — Rust's sorted arrangement.
fn data_row_ids(html: &str) -> Vec<usize> {
    let pat = "data-row=\"";
    let mut ids = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(pat) {
        let after = &rest[i + pat.len()..];
        let end = after.find('"').expect("data-row attr closes");
        ids.push(after[..end].parse().expect("numeric data-row id"));
        rest = &after[end..];
    }
    ids
}

/// Build the fixture object for one sort config: `{ keys, payload, rustOrder }`.
fn build_fixture(keys: &[(&str, bool)]) -> J {
    let base = build_base(keys);
    let corpus = corpus();
    let html = render_base(&base, &corpus, &interactive_opts());

    let payload = extract_payload(&html);
    let rust_order = data_row_ids(&html);

    // Emitter self-consistency: the DOM `data-row` order MUST equal the payload
    // `rows[].id` order (both are Rust's post-sort arrangement).
    let payload_ids: Vec<usize> = payload["rows"]
        .as_array()
        .expect("rows is an array")
        .iter()
        .map(|r| r["id"].as_u64().expect("id is a number") as usize)
        .collect();
    assert_eq!(
        rust_order, payload_ids,
        "data-row sequence must equal payload rows[].id order for keys {keys:?}"
    );

    let keys_json: Vec<J> = keys
        .iter()
        .map(|(col, desc)| json!({ "col": col, "desc": desc }))
        .collect();

    json!({
        "keys": keys_json,
        "payload": payload,
        "rustOrder": rust_order,
    })
}

// ---------------------------------------------------------------------------
// Fixture directory + guard
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docgen-assets")
        .join("js-tests")
        .join("fixtures")
}

/// Pretty, deterministic JSON so re-blessing produces a byte-identical file (the
/// payload objects are BTreeMap-backed, so key order is stable).
fn to_pretty(v: &J) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("serialize fixture");
    s.push('\n');
    s
}

#[test]
fn sort_parity_fixtures() {
    let bless = std::env::var("BLESS").map(|v| v == "1").unwrap_or(false);
    let dir = fixtures_dir();

    if bless {
        std::fs::create_dir_all(&dir).expect("create fixtures dir");
    }

    // Track which files are ours, so a stale fixture can't linger unnoticed.
    let mut expected_files: Vec<String> = Vec::new();

    for (i, keys) in configs().iter().enumerate() {
        let name = format!("sort-{i}.json");
        expected_files.push(name.clone());
        let path = dir.join(&name);
        let fixture = build_fixture(keys);

        if bless {
            std::fs::write(&path, to_pretty(&fixture))
                .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            eprintln!("blessed {}", path.display());
            continue;
        }

        // Guard mode: the committed fixture must exist and match exactly.
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing fixture {} — run `BLESS=1 cargo test -p docgen-bases --test sort_parity` to create it",
                path.display()
            )
        });
        let committed: J = serde_json::from_str(&committed)
            .unwrap_or_else(|e| panic!("fixture {} is not valid JSON: {e}", path.display()));
        assert_eq!(
            committed,
            fixture,
            "Rust side drifted: committed fixture {} no longer matches the regenerated \
             payload/order. If this change is intended, re-bless with \
             `BLESS=1 cargo test -p docgen-bases --test sort_parity`.",
            path.display()
        );
    }

    // Ensure no orphaned sort-*.json fixtures remain (only meaningful in guard
    // mode; blessing a smaller set should still flag leftovers).
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut orphans: BTreeMap<String, ()> = BTreeMap::new();
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.starts_with("sort-")
                && fname.ends_with(".json")
                && !expected_files.contains(&fname)
            {
                orphans.insert(fname, ());
            }
        }
        assert!(
            orphans.is_empty(),
            "orphaned sort fixtures (not produced by any config): {:?}",
            orphans.keys().collect::<Vec<_>>()
        );
    }
}
