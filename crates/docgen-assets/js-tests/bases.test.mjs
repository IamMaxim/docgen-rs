// Dev-only Node parity tests for the interactive-bases island pure logic.
//
// These verify that the browser island's sort/filter/search/facet/state functions
// reproduce the Rust semantics in crates/docgen-bases/src/value.rs (loose_cmp,
// as_number, is_empty, BaseDate::epoch_millis) exactly. Only the *pure* functions
// are exercised — no DOM.
//
// RUN (from repo root):
//   node --test crates/docgen-assets/assets/docgen/islands/__tests__/
//
// The island exposes its pure fns via a `module.exports` tail that stays invisible
// in the browser (`module`/`document` undefined). We import that CommonJS module
// from this ESM test file via createRequire.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const B = require('../assets/docgen/islands/bases.js');

// --- helpers ----------------------------------------------------------------
const num = (n) => ({ t: 'num', d: String(n), num: n });
const str = (s) => ({ t: 'str', d: s });
const bool = (b) => ({ t: 'bool', d: String(b), num: b ? 1 : 0 });
const date = (epoch, d) => ({ t: 'date', d: d || String(epoch), epoch });
const list = (arr) => ({ t: 'list', d: arr.join(', '), f: arr });
const link = (s) => ({ t: 'link', d: s });
const nul = () => ({ t: 'null', d: '', empty: true });
// numeric string cell that carries `num` (as in the payload for a coercible str)
const numstr = (s) => ({ t: 'str', d: s, num: Number(s) });

// ============================================================================
// compareCells — parity with loose_cmp
// ============================================================================
test('compareCells: num < num', () => {
  assert.equal(B.compareCells(num(1), num(2)), -1);
  assert.equal(B.compareCells(num(2), num(1)), 1);
  assert.equal(B.compareCells(num(2), num(2)), 0);
});

test('compareCells: date by epoch', () => {
  assert.equal(B.compareCells(date(100), date(200)), -1);
  assert.equal(B.compareCells(date(200), date(100)), 1);
  assert.equal(B.compareCells(date(200), date(200)), 0);
});

test('compareCells: str case-insensitive', () => {
  assert.equal(B.compareCells(str('apple'), str('Banana')), -1);
  assert.equal(B.compareCells(str('Banana'), str('apple')), 1);
  assert.equal(B.compareCells(str('ABC'), str('abc')), 0);
});

test('compareCells: link case-insensitive by display', () => {
  assert.equal(B.compareCells(link('Alice'), link('bob')), -1);
  assert.equal(B.compareCells(link('bob'), link('Alice')), 1);
});

test('compareCells: null sorts last (ascending)', () => {
  assert.equal(B.compareCells(nul(), num(1)), 1);
  assert.equal(B.compareCells(num(1), nul()), -1);
  assert.equal(B.compareCells(nul(), nul()), 0);
  // a missing cell (undefined) is also null
  assert.equal(B.compareCells(undefined, num(1)), 1);
  assert.equal(B.compareCells(num(1), undefined), -1);
});

test('compareCells: bool false < true', () => {
  assert.equal(B.compareCells(bool(false), bool(true)), -1);
  assert.equal(B.compareCells(bool(true), bool(false)), 1);
});

test('compareCells: cross-type numeric coercion (num vs numeric-string with num)', () => {
  // Rust `_` arm: both have as_number → compare numerically.
  assert.equal(B.compareCells(num(3), numstr('10')), -1);
  assert.equal(B.compareCells(numstr('10'), num(3)), 1);
  assert.equal(B.compareCells(num(5), numstr('5')), 0);
});

test('compareCells: cross-type date vs string falls to display (date has no num)', () => {
  // date epoch must NOT be used in the generic branch; compare display strings.
  // date display "2020" vs string "abc": '2020' < 'abc' lexicographically.
  assert.equal(B.compareCells(date(0, '2020'), str('abc')), -1);
  assert.equal(B.compareCells(str('abc'), date(0, '2020')), 1);
  // Prove epoch is ignored: a huge-epoch date with display '2020' still < 'abc'.
  assert.equal(B.compareCells(date(9e15, '2020'), str('abc')), -1);
});

test('compareCells: list vs list by display', () => {
  assert.equal(B.compareCells(list(['a', 'b']), list(['c'])), -1);
  assert.equal(B.compareCells(list(['c']), list(['a', 'b'])), 1);
});

// ============================================================================
// sortIds — stable, desc negation, multi-key, id tiebreak
// ============================================================================
function byId(rows) {
  const m = {};
  for (const r of rows) m[r.id] = r;
  return m;
}

test('sortIds: ascending with null last', () => {
  const rows = [
    { id: 0, cells: { n: num(3) } },
    { id: 1, cells: { n: nul() } },
    { id: 2, cells: { n: num(1) } }
  ];
  assert.deepEqual(
    B.sortIds([0, 1, 2], byId(rows), [{ col: 'n', desc: false }]),
    [2, 0, 1]
  );
});

test('sortIds: desc negation (matches Rust ord.reverse(): null sorts first desc)', () => {
  const rows = [
    { id: 0, cells: { n: num(3) } },
    { id: 1, cells: { n: nul() } },
    { id: 2, cells: { n: num(1) } }
  ];
  // Rust apply_sort negates the WHOLE loose_cmp under desc, incl. the null-last
  // branch, so null moves to the front descending: null, 3, 1.
  assert.deepEqual(
    B.sortIds([0, 1, 2], byId(rows), [{ col: 'n', desc: true }]),
    [1, 0, 2]
  );
});

test('sortIds: stable id tiebreak on equal keys', () => {
  const rows = [
    { id: 5, cells: { n: num(1) } },
    { id: 2, cells: { n: num(1) } },
    { id: 9, cells: { n: num(1) } }
  ];
  assert.deepEqual(
    B.sortIds([5, 2, 9], byId(rows), [{ col: 'n', desc: false }]),
    [2, 5, 9]
  );
});

test('sortIds: multi-key', () => {
  const rows = [
    { id: 0, cells: { a: str('x'), b: num(2) } },
    { id: 1, cells: { a: str('x'), b: num(1) } },
    { id: 2, cells: { a: str('a'), b: num(9) } }
  ];
  assert.deepEqual(
    B.sortIds([0, 1, 2], byId(rows), [
      { col: 'a', desc: false },
      { col: 'b', desc: false }
    ]),
    [2, 1, 0]
  );
});

// ============================================================================
// facetTokens / deriveFacets
// ============================================================================
test('facetTokens: scalar -> [d]; empty -> (empty); list -> f; empty list -> (empty)', () => {
  assert.deepEqual(B.facetTokens(str('backend')), ['backend']);
  assert.deepEqual(B.facetTokens(nul()), ['(empty)']);
  assert.deepEqual(B.facetTokens(undefined), ['(empty)']);
  assert.deepEqual(B.facetTokens(list(['api', 'db'])), ['api', 'db']);
  assert.deepEqual(B.facetTokens({ t: 'list', d: '' }), ['(empty)']);
});

test('deriveFacets: frequency desc, then token asc, (empty) always last', () => {
  const rows = [
    { id: 0, cells: { s: str('b') } },
    { id: 1, cells: { s: str('a') } },
    { id: 2, cells: { s: str('a') } },
    { id: 3, cells: { s: nul() } },
    { id: 4, cells: { s: str('c') } },
    { id: 5, cells: { s: str('c') } }
  ];
  // counts: a=2, c=2, b=1, (empty)=1 -> a,c (asc within tie), b, then (empty) last
  assert.deepEqual(B.deriveFacets(rows, 's'), [
    { token: 'a', count: 2 },
    { token: 'c', count: 2 },
    { token: 'b', count: 1 },
    { token: '(empty)', count: 1 }
  ]);
});

// ============================================================================
// matchRow — search AND, enum OR-within/AND-across, ranges, empty selects nulls
// ============================================================================
const COLS = [
  { key: 'title' },
  { key: 'tag' },
  { key: 'when' },
  { key: 'score' }
];

function row(id, cells) {
  return { id, cells };
}

test('matchRow: search AND across tokens over joined haystack', () => {
  const r = row(0, { title: str('Hello World'), tag: str('api') });
  const cols = [{ key: 'title' }, { key: 'tag' }];
  assert.equal(B.matchRow(r, { search: 'hello api' }, cols), true);
  assert.equal(B.matchRow(r, { search: 'hello missing' }, cols), false);
  assert.equal(B.matchRow(r, { search: '' }, cols), true);
});

test('matchRow: enum OR within a column, AND across columns', () => {
  const r = row(0, { tag: list(['api', 'db']), title: str('svc-a') });
  const cols = [{ key: 'tag' }, { key: 'title' }];
  // OR within tag: matches api OR web -> api present
  assert.equal(
    B.matchRow(r, { facets: { tag: ['web', 'api'] } }, cols),
    true
  );
  // AND across: tag ok but title requires svc-b -> fail
  assert.equal(
    B.matchRow(
      r,
      { facets: { tag: ['api'], title: ['svc-b'] } },
      cols
    ),
    false
  );
  // both satisfied
  assert.equal(
    B.matchRow(
      r,
      { facets: { tag: ['api'], title: ['svc-a'] } },
      cols
    ),
    true
  );
});

test('matchRow: (empty) facet token selects null/empty cells', () => {
  const rEmpty = row(0, { tag: nul() });
  const rFull = row(1, { tag: str('api') });
  const cols = [{ key: 'tag' }];
  assert.equal(B.matchRow(rEmpty, { facets: { tag: ['(empty)'] } }, cols), true);
  assert.equal(B.matchRow(rFull, { facets: { tag: ['(empty)'] } }, cols), false);
});

test('matchRow: date range inclusive boundaries', () => {
  const cols = [{ key: 'when' }];
  const r = row(0, { when: date(1000) });
  // inclusive on both bounds
  assert.equal(B.matchRow(r, { dates: { when: { from: 1000, to: 2000 } } }, cols), true);
  assert.equal(B.matchRow(r, { dates: { when: { from: 500, to: 1000 } } }, cols), true);
  // exactly on `to`
  assert.equal(B.matchRow(r, { dates: { when: { to: 1000 } } }, cols), true);
  // just outside
  assert.equal(B.matchRow(r, { dates: { when: { from: 1001 } } }, cols), false);
  assert.equal(B.matchRow(r, { dates: { when: { to: 999 } } }, cols), false);
  // non-date / empty cell fails when a bound is set
  const rNull = row(1, { when: nul() });
  assert.equal(B.matchRow(rNull, { dates: { when: { from: 0 } } }, cols), false);
});

test('matchRow: number range inclusive; missing num fails when bound set', () => {
  const cols = [{ key: 'score' }];
  const r = row(0, { score: num(5) });
  assert.equal(B.matchRow(r, { numbers: { score: { from: 5, to: 10 } } }, cols), true);
  assert.equal(B.matchRow(r, { numbers: { score: { from: 1, to: 5 } } }, cols), true);
  assert.equal(B.matchRow(r, { numbers: { score: { from: 6 } } }, cols), false);
  const rStr = row(1, { score: str('nope') });
  assert.equal(B.matchRow(rStr, { numbers: { score: { from: 0 } } }, cols), false);
});

test('matchRow: empty selects (no filters) always matches', () => {
  const r = row(0, { title: str('x') });
  assert.equal(B.matchRow(r, {}, COLS), true);
  // an empty facet array is a no-op
  assert.equal(B.matchRow(r, { facets: { title: [] } }, COLS), true);
});

// ============================================================================
// encodeState / decodeState round-trip
// ============================================================================
function rt(state) {
  return B.decodeState(B.encodeState(state));
}

test('encodeState/decodeState: empty round-trips to defaults', () => {
  const s = { search: '', facets: {}, dates: {}, numbers: {}, sort: [], page: 0 };
  assert.deepEqual(rt(s), s);
  assert.equal(B.encodeState(s), '');
});

test('encodeState/decodeState: full state round-trip', () => {
  const s = {
    search: 'hello world',
    facets: { 'note.tag': ['api', 'db'], service: ['backend'] },
    dates: { 'note.due': { from: 1000, to: 2000 } },
    numbers: { score: { from: 1 } },
    sort: [
      { col: 'note.due', desc: true },
      { col: 'score', desc: false }
    ],
    page: 3
  };
  assert.deepEqual(rt(s), s);
});

test('encodeState/decodeState: special chars in tokens survive', () => {
  const s = {
    search: 'a & b = c',
    facets: { col: ['a~b', 'x&y=z', 'c,d', 'e:f', 'g.h', 'space here'] },
    dates: {},
    numbers: {},
    sort: [],
    page: 0
  };
  assert.deepEqual(rt(s), s);
});

test('encodeState/decodeState: one-sided ranges round-trip', () => {
  const s = {
    search: '',
    facets: {},
    dates: { d: { from: 42 } },
    numbers: { n: { to: 99 } },
    sort: [],
    page: 0
  };
  assert.deepEqual(rt(s), s);
});
