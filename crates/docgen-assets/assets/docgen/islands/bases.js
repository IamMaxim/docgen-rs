// docgen interactive-bases island — M4 (full client-side behavior).
//
// Consumes the v1 JSON payload emitted by docgen-bases (render.rs / interactive.rs)
// and progressively enhances the *existing* SSR DOM: filter, search, sort/reorder,
// date & number ranges, faceted enums, pagination, and shareable URL state — across
// table / cards / list views. It NEVER re-renders cell content: rows are reordered by
// moving their existing nodes and hidden with the `hidden` attribute, so what a cell
// looks like stays 100% Rust-rendered (zero Rust-vs-JS divergence). Any parse/version
// failure leaves the static HTML fully intact (progressive enhancement).
//
// See .overnight/interactive-bases/{SCHEMA.md,M4-SPEC.md} for the frozen contract.
//
// The comparator/sort/filter *pure* functions mirror crates/docgen-bases/src/value.rs
// (loose_cmp / as_number / is_empty / BaseDate::epoch_millis) exactly — the payload
// pre-computes `num` (as_number) and `epoch` (epoch_millis) so the JS never re-parses.
//
// ============================================================================
// CLASS-NAME / DOM CONTRACT (for M6 styling — all structure, no styles here)
// ============================================================================
// Inserted control bar (between the view title and the table/cards/list body):
//   .docgen-base-controls           role="group" — the whole control bar
//   .docgen-base-search             <input type="search"> free-text search
//   .docgen-base-search-label       visually-hidden <label> wrapping the input
//   .docgen-base-facet              a faceted-enum filter (button + panel) wrapper
//   .docgen-base-facet__button      <button aria-expanded> that opens the panel
//   .docgen-base-facet__badge       <span> active-selection count badge (hidden when 0)
//   .docgen-base-facet__panel       popover <div> of checkboxes (hidden until open)
//   .docgen-base-facet__option      <label> wrapping one checkbox + token + count
//   .docgen-base-daterange          a date-range filter wrapper (two date inputs)
//   .docgen-base-numrange           a number-range filter wrapper (two number inputs)
//   .docgen-base-range__from / __to the individual range <input>s
//   .docgen-base-boolean            a boolean tri-state <select> (Any/True/False)
//   .docgen-base-sort               cards/list sort <select> (Default + col×asc/desc)
//   .docgen-base-reset              <button> clears all state
//   .docgen-base-status             aria-live="polite" "{n} of {total}" region
//   .docgen-base-pager              pager wrapper (hidden when pageSize=0)
//   .docgen-base-pager__prev / __next   pager buttons
//   .docgen-base-pager__label       "x–y of N" span
//   .docgen-base-field              generic wrapper around a label+control group
//   .docgen-base-field__label       the (mostly visually-hidden) label text
// Table header sort affordance:
//   button.docgen-base-sort-th      wraps the <th> header text; <th aria-sort=…>
// Injected empty-state (when a filter yields 0 rows, if none present in SSR):
//   tr.docgen-base-empty-row > td.docgen-base-empty   (table)
//   .docgen-base-empty              (cards: div, list: li)
// Section flags: data-base-hydrated="1" once enhanced.
// ============================================================================

(function () {
  'use strict';

  // ==========================================================================
  // ===== PURE LOGIC (unit-tested under Node; see __tests__/bases.test.mjs) ===
  // ==========================================================================

  // A missing cell (column key absent in a row) is treated as NULL.
  function isNull(c) {
    return !c || c.t === 'null';
  }
  function cmpNum(a, b) {
    return a < b ? -1 : a > b ? 1 : 0;
  }
  function cmpStr(a, b) {
    a = (a == null ? '' : String(a)).toLowerCase();
    b = (b == null ? '' : String(b)).toLowerCase();
    return a < b ? -1 : a > b ? 1 : 0;
  }

  // Mirrors Value::loose_cmp EXACTLY. Null sorts LAST (ascending).
  function compareCells(a, b) {
    var an = isNull(a),
      bn = isNull(b);
    if (an && bn) return 0;
    if (an) return 1;
    if (bn) return -1; // null last
    if (a.t === b.t) {
      switch (a.t) {
        case 'num':
          return cmpNum(a.num, b.num);
        case 'date':
          return cmpNum(a.epoch, b.epoch);
        case 'dur':
          return cmpNum(a.num, b.num);
        case 'bool':
          return cmpNum(a.num, b.num);
        case 'str':
          return cmpStr(a.d, b.d);
        case 'link':
          return cmpStr(a.d, b.d);
        // list/obj: fall through to the generic branch (matches Rust `_` arm).
      }
    }
    // Rust `_`: numeric coercion iff BOTH have as_number (== payload `num`); else display.
    // NOTE: date has no `num` (only `epoch`), so cross-type date compares by display.
    if (
      a.num !== undefined &&
      a.num !== null &&
      b.num !== undefined &&
      b.num !== null
    ) {
      return cmpNum(a.num, b.num);
    }
    return cmpStr(a.d, b.d);
  }

  // Stable multi-key sort of row ids. `rowsById[id] = {id, cells}`.
  // sortKeys = [{col, desc}]. Final tiebreak by ascending id → total & reproducible.
  function sortIds(ids, rowsById, sortKeys) {
    var keys = sortKeys || [];
    var arr = ids.slice();
    arr.sort(function (x, y) {
      for (var i = 0; i < keys.length; i++) {
        var k = keys[i];
        var rx = rowsById[x],
          ry = rowsById[y];
        var cx = rx && rx.cells ? rx.cells[k.col] : undefined;
        var cy = ry && ry.cells ? ry.cells[k.col] : undefined;
        var c = compareCells(cx, cy);
        if (k.desc) c = -c;
        if (c) return c;
      }
      return x - y; // id tiebreak (ascending)
    });
    return arr;
  }

  // Facet tokens for one cell.
  //  missing / empty / t:"null"  -> ['(empty)']
  //  t:"list"                    -> cell.f if non-empty, else ['(empty)']
  //  scalar                      -> [cell.d]
  function facetTokens(c) {
    if (!c || c.t === 'null' || c.empty) return ['(empty)'];
    if (c.t === 'list') return c.f && c.f.length ? c.f.slice() : ['(empty)'];
    return [c.d];
  }

  // deriveFacets(rows, col) -> [{token, count}] sorted by count desc, then token
  // asc (case-insensitive). '(empty)' always sorts last regardless of count.
  function deriveFacets(rows, col) {
    var counts = {};
    for (var i = 0; i < rows.length; i++) {
      var toks = facetTokens(rows[i].cells ? rows[i].cells[col] : undefined);
      for (var j = 0; j < toks.length; j++) {
        var t = toks[j];
        counts[t] = (counts[t] || 0) + 1;
      }
    }
    var arr = Object.keys(counts).map(function (t) {
      return { token: t, count: counts[t] };
    });
    arr.sort(function (a, b) {
      var ae = a.token === '(empty)',
        be = b.token === '(empty)';
      if (ae && !be) return 1;
      if (be && !ae) return -1;
      if (a.count !== b.count) return b.count - a.count;
      var al = a.token.toLowerCase(),
        bl = b.token.toLowerCase();
      return al < bl ? -1 : al > bl ? 1 : 0;
    });
    return arr;
  }

  // The row's searchable text: visible cells' `d` joined by ' ', lowercased.
  function rowHaystack(row, visibleCols) {
    var parts = [];
    for (var i = 0; i < visibleCols.length; i++) {
      var c = row.cells ? row.cells[visibleCols[i]] : undefined;
      parts.push(c && c.d != null ? c.d : '');
    }
    return parts.join(' ').toLowerCase();
  }

  // Row matches iff every whitespace-split query token is a substring (AND).
  function matchSearch(hay, query) {
    if (!query) return true;
    var toks = String(query).toLowerCase().split(/\s+/);
    for (var i = 0; i < toks.length; i++) {
      if (!toks[i]) continue;
      if (hay.indexOf(toks[i]) < 0) return false;
    }
    return true;
  }

  // AND across: search, each facet column (OR within), each date range, each number range.
  function matchRow(row, state, columns) {
    var i;
    if (state.search) {
      var cols = [];
      for (i = 0; i < columns.length; i++) cols.push(columns[i].key);
      if (!matchSearch(rowHaystack(row, cols), state.search)) return false;
    }
    var facets = state.facets || {};
    for (var fcol in facets) {
      if (!Object.prototype.hasOwnProperty.call(facets, fcol)) continue;
      var sel = facets[fcol];
      if (!sel || !sel.length) continue;
      var toks = facetTokens(row.cells ? row.cells[fcol] : undefined);
      var hit = false;
      for (i = 0; i < toks.length; i++) {
        if (sel.indexOf(toks[i]) >= 0) {
          hit = true;
          break;
        }
      }
      if (!hit) return false;
    }
    var dates = state.dates || {};
    for (var dcol in dates) {
      if (!Object.prototype.hasOwnProperty.call(dates, dcol)) continue;
      var dr = dates[dcol];
      if (!dr || (dr.from == null && dr.to == null)) continue;
      var dc = row.cells ? row.cells[dcol] : undefined;
      if (!dc || dc.t !== 'date' || dc.epoch == null) return false;
      if (dr.from != null && dc.epoch < dr.from) return false;
      if (dr.to != null && dc.epoch > dr.to) return false;
    }
    var numbers = state.numbers || {};
    for (var ncol in numbers) {
      if (!Object.prototype.hasOwnProperty.call(numbers, ncol)) continue;
      var nr = numbers[ncol];
      if (!nr || (nr.from == null && nr.to == null)) continue;
      var nc = row.cells ? row.cells[ncol] : undefined;
      if (!nc || nc.num == null) return false;
      if (nr.from != null && nc.num < nr.from) return false;
      if (nr.to != null && nc.num > nr.to) return false;
    }
    return true;
  }

  // --- URL state (de)serialization ------------------------------------------
  // encodeURIComponent leaves `~` unescaped, but we join facet tokens with `~`,
  // so escape it too; decodeURIComponent restores it.
  function enc(s) {
    return encodeURIComponent(String(s)).replace(/~/g, '%7E');
  }
  function dec(s) {
    try {
      return decodeURIComponent(s);
    } catch (e) {
      return s;
    }
  }

  function encodeState(state) {
    var parts = [];
    if (state.search) parts.push('q=' + enc(state.search));
    var facets = state.facets || {};
    Object.keys(facets)
      .sort()
      .forEach(function (col) {
        var toks = facets[col];
        if (toks && toks.length) {
          parts.push('f.' + enc(col) + '=' + toks.map(enc).join('~'));
        }
      });
    var dates = state.dates || {};
    Object.keys(dates)
      .sort()
      .forEach(function (col) {
        var r = dates[col];
        if (!r) return;
        var from = r.from == null ? '' : r.from,
          to = r.to == null ? '' : r.to;
        if (from !== '' || to !== '')
          parts.push('d.' + enc(col) + '=' + from + '..' + to);
      });
    var numbers = state.numbers || {};
    Object.keys(numbers)
      .sort()
      .forEach(function (col) {
        var r = numbers[col];
        if (!r) return;
        var from = r.from == null ? '' : r.from,
          to = r.to == null ? '' : r.to;
        if (from !== '' || to !== '')
          parts.push('n.' + enc(col) + '=' + from + '..' + to);
      });
    var sort = state.sort || [];
    if (sort.length) {
      parts.push(
        's=' +
          sort
            .map(function (k) {
              return enc(k.col) + ':' + (k.desc ? 'desc' : 'asc');
            })
            .join(',')
      );
    }
    if (state.page) parts.push('pg=' + state.page);
    return parts.join('&');
  }

  function parseRange(raw) {
    var i = raw.indexOf('..');
    var from, to;
    if (i < 0) {
      from = raw;
      to = '';
    } else {
      from = raw.slice(0, i);
      to = raw.slice(i + 2);
    }
    var r = {};
    if (from !== '') r.from = Number(from);
    if (to !== '') r.to = Number(to);
    return r;
  }

  function decodeState(str) {
    var s = { search: '', facets: {}, dates: {}, numbers: {}, sort: [], page: 0 };
    if (!str) return s;
    String(str)
      .split('&')
      .forEach(function (p) {
        if (!p) return;
        var eq = p.indexOf('=');
        var key = eq < 0 ? p : p.slice(0, eq);
        var raw = eq < 0 ? '' : p.slice(eq + 1);
        if (key === 'q') {
          s.search = dec(raw);
        } else if (key === 's') {
          s.sort = raw
            .split(',')
            .filter(Boolean)
            .map(function (seg) {
              var c = seg.split(':');
              return { col: dec(c[0]), desc: c[1] === 'desc' };
            });
        } else if (key === 'pg') {
          s.page = parseInt(raw, 10) || 0;
        } else if (key.slice(0, 2) === 'f.') {
          s.facets[dec(key.slice(2))] = raw.split('~').map(dec);
        } else if (key.slice(0, 2) === 'd.') {
          s.dates[dec(key.slice(2))] = parseRange(raw);
        } else if (key.slice(0, 2) === 'n.') {
          s.numbers[dec(key.slice(2))] = parseRange(raw);
        }
      });
    return s;
  }

  // ==========================================================================
  // ===== DOM LAYER ==========================================================
  // ==========================================================================

  var VIEWS = []; // hydrated views, for global hashchange handling
  var loggedParseError = false;
  var loggedVersion = false;

  function h(tag, cls, attrs) {
    var el = document.createElement(tag);
    if (cls) el.className = cls;
    if (attrs)
      for (var k in attrs)
        if (Object.prototype.hasOwnProperty.call(attrs, k))
          el.setAttribute(k, attrs[k]);
    return el;
  }

  function visuallyHidden(el) {
    el.style.position = 'absolute';
    el.style.width = '1px';
    el.style.height = '1px';
    el.style.padding = '0';
    el.style.margin = '-1px';
    el.style.overflow = 'hidden';
    el.style.clip = 'rect(0 0 0 0)';
    el.style.whiteSpace = 'nowrap';
    el.style.border = '0';
    return el;
  }

  // Convert a <input type=date> "YYYY-MM-DD" to a UTC epoch (start or end of day).
  function dateInputToEpoch(val, endOfDay) {
    if (!val) return null;
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(val);
    if (!m) return null;
    return Date.UTC(
      +m[1],
      +m[2] - 1,
      +m[3],
      endOfDay ? 23 : 0,
      endOfDay ? 59 : 0,
      endOfDay ? 59 : 0,
      endOfDay ? 999 : 0
    );
  }
  function epochToDateInput(epoch) {
    if (epoch == null) return '';
    try {
      return new Date(epoch).toISOString().slice(0, 10);
    } catch (e) {
      return '';
    }
  }

  function makeState(data) {
    var sort = (data.controls && data.controls.sort ? data.controls.sort : []).map(
      function (k) {
        return { col: k.col, desc: !!k.desc };
      }
    );
    return { search: '', facets: {}, dates: {}, numbers: {}, sort: sort, page: 0 };
  }

  function hydrate(section) {
    var idx = section.getAttribute('data-base-view');
    var tag = section.querySelector('script.docgen-base-data');
    if (!tag) return;
    var data;
    try {
      data = JSON.parse(tag.textContent);
    } catch (e) {
      if (!loggedParseError) {
        loggedParseError = true;
        console.error('[docgen bases] failed to parse payload', e);
      }
      return; // leave static HTML untouched
    }
    if (!data || data.v !== 1) {
      if (!loggedVersion) {
        loggedVersion = true;
        console.warn(
          '[docgen bases] unsupported payload version; leaving static HTML',
          data && data.v
        );
      }
      return;
    }

    var container =
      section.querySelector('tbody') ||
      section.querySelector('.docgen-base-cards') ||
      section.querySelector('.docgen-base-list');
    if (!container) return;

    var rowsById = {};
    for (var i = 0; i < data.rows.length; i++) rowsById[data.rows[i].id] = data.rows[i];

    var nodeById = {};
    var dataNodes = container.querySelectorAll('[data-row]');
    for (i = 0; i < dataNodes.length; i++) {
      var id = parseInt(dataNodes[i].getAttribute('data-row'), 10);
      nodeById[id] = dataNodes[i];
    }

    var colByKey = {};
    for (i = 0; i < data.columns.length; i++) colByKey[data.columns[i].key] = data.columns[i];

    var V = {
      section: section,
      idx: idx,
      data: data,
      view: data.view || {},
      columns: data.columns || [],
      colByKey: colByKey,
      rows: data.rows || [],
      rowsById: rowsById,
      nodeById: nodeById,
      container: container,
      grouped: !!(data.view && data.view.groupBy),
      controls: {}, // {facetPanels, updaters:[]}
      updaters: [], // fns that push state -> widget values
      state: makeState(data),
      allIds: (data.rows || []).map(function (r) {
        return r.id;
      }),
      // The SSR DOM is already emitted in the default (payload) sort order, so the
      // first render only reorders if the URL restores a different sort.
      domOrderSig: null,
      prevVisible: null
    };
    V.domOrderSig = sortSig(
      (data.controls && data.controls.sort ? data.controls.sort : []).map(function (k) {
        return { col: k.col, desc: !!k.desc };
      })
    );
    // Seed the visibility baseline to the ACTUAL initial DOM state: the SSR emits
    // every row visible (no `hidden`). Starting from {} would make the first
    // render's delta skip rows that must become hidden (want=false, had=false).
    V.prevVisible = {};
    for (var ai = 0; ai < V.allIds.length; ai++) V.prevVisible[V.allIds[ai]] = true;

    buildControlBar(V);
    if (!V.grouped) wireSort(V);

    restoreFromUrl(V);
    syncControlsFromState(V);
    applyAndRender(V);

    section.dataset.baseHydrated = '1';
    VIEWS.push(V);
  }

  function buildControlBar(V) {
    var data = V.data;
    var viewName = (V.view && V.view.name) || 'view';
    var bar = h('div', 'docgen-base-controls', {
      role: 'group',
      'aria-label': 'Filter and sort ' + viewName
    });

    // Search
    if (data.controls && data.controls.search) {
      var sLabelWrap = h('label', 'docgen-base-search-label');
      var sText = h('span', 'docgen-base-field__label');
      sText.textContent = 'Search ' + viewName;
      visuallyHidden(sText);
      var sInput = h('input', 'docgen-base-search', {
        type: 'search',
        placeholder: 'Search ' + viewName + '…'
      });
      sLabelWrap.appendChild(sText);
      sLabelWrap.appendChild(sInput);
      bar.appendChild(sLabelWrap);
      sInput.addEventListener('input', function () {
        V.state.search = sInput.value;
        V.state.page = 0;
        applyAndRender(V);
      });
      V.updaters.push(function () {
        if (sInput.value !== V.state.search) sInput.value = V.state.search;
      });
    }

    // Per-column widgets
    for (var i = 0; i < V.columns.length; i++) {
      var col = V.columns[i];
      switch (col.filter) {
        case 'enum':
          buildFacet(V, bar, col, deriveFacets(V.rows, col.key));
          break;
        case 'boolean':
          buildBoolean(V, bar, col);
          break;
        case 'date':
          buildDateRange(V, bar, col);
          break;
        case 'number':
          buildNumberRange(V, bar, col);
          break;
        // 'text' and 'none': covered by global search; no per-column widget.
      }
    }

    // Sort select for cards/list (no clickable headers). Skip if grouped.
    if (V.view.type !== 'table' && !V.grouped) buildSortSelect(V, bar);

    // Reset
    var reset = h('button', 'docgen-base-reset', { type: 'button' });
    reset.textContent = 'Reset';
    reset.addEventListener('click', function () {
      V.state = makeState(V.data);
      syncControlsFromState(V);
      applyAndRender(V);
    });
    bar.appendChild(reset);

    // Status (aria-live)
    V.statusEl = h('div', 'docgen-base-status', {
      role: 'status',
      'aria-live': 'polite'
    });
    bar.appendChild(V.statusEl);

    // Pager (only when paginating)
    var pageSize = (data.controls && data.controls.pageSize) || 0;
    if (pageSize > 0) {
      var pager = h('div', 'docgen-base-pager');
      var prev = h('button', 'docgen-base-pager__prev', { type: 'button' });
      prev.textContent = 'Prev';
      var label = h('span', 'docgen-base-pager__label');
      var next = h('button', 'docgen-base-pager__next', { type: 'button' });
      next.textContent = 'Next';
      prev.addEventListener('click', function () {
        if (V.state.page > 0) {
          V.state.page--;
          applyAndRender(V);
        }
      });
      next.addEventListener('click', function () {
        V.state.page++;
        applyAndRender(V);
      });
      pager.appendChild(prev);
      pager.appendChild(label);
      pager.appendChild(next);
      bar.appendChild(pager);
      V.pager = { el: pager, prev: prev, next: next, label: label };
    }

    // Insert the bar before the body (table-scroll / cards / list).
    var body =
      V.section.querySelector('.docgen-table-scroll') ||
      V.section.querySelector('.docgen-base-cards') ||
      V.section.querySelector('.docgen-base-list');
    if (body && body.parentNode) body.parentNode.insertBefore(bar, body);
    else V.section.appendChild(bar);
    V.bar = bar;
  }

  function buildFacet(V, bar, col, facets) {
    var wrap = h('div', 'docgen-base-facet');
    // Unique per-view id: i2id(key) can collide for keys differing only in
    // special chars, so append a per-view sequence to guarantee distinct ids.
    V.facetSeq = (V.facetSeq || 0) + 1;
    var panelId = 'docgen-facet-' + V.idx + '-' + i2id(col.key) + '-' + V.facetSeq;
    var btn = h('button', 'docgen-base-facet__button', {
      type: 'button',
      'aria-expanded': 'false',
      'aria-controls': panelId
    });
    btn.textContent = col.header + ' ';
    // The count badge is decorative; keep it out of the button's accessible name
    // (otherwise it reads as e.g. "Status 2"). The selection count is conveyed via
    // the button's aria-label in refreshBadge instead.
    var badge = h('span', 'docgen-base-facet__badge', { 'aria-hidden': 'true' });
    badge.hidden = true;
    btn.appendChild(badge);
    btn.setAttribute('aria-label', col.header);
    var panel = h('div', 'docgen-base-facet__panel', { id: panelId });
    panel.hidden = true;

    var boxes = [];
    facets.forEach(function (f) {
      var opt = h('label', 'docgen-base-facet__option');
      var box = h('input', null, { type: 'checkbox', value: f.token });
      var txt = document.createElement('span');
      txt.textContent = ' ' + f.token + ' (' + f.count + ')';
      opt.appendChild(box);
      opt.appendChild(txt);
      panel.appendChild(opt);
      boxes.push(box);
      box.addEventListener('change', function () {
        var sel = [];
        boxes.forEach(function (b) {
          if (b.checked) sel.push(b.value);
        });
        if (sel.length) V.state.facets[col.key] = sel;
        else delete V.state.facets[col.key];
        V.state.page = 0;
        applyAndRender(V);
        refreshBadge();
      });
    });

    function refreshBadge() {
      var sel = V.state.facets[col.key] || [];
      if (sel.length) {
        badge.hidden = false;
        badge.textContent = String(sel.length);
        btn.setAttribute('aria-label', col.header + ', ' + sel.length + ' selected');
      } else {
        badge.hidden = true;
        badge.textContent = '';
        btn.setAttribute('aria-label', col.header);
      }
    }

    function open() {
      panel.hidden = false;
      btn.setAttribute('aria-expanded', 'true');
      document.addEventListener('click', outside, true);
      document.addEventListener('keydown', onEsc, true);
    }
    function close() {
      panel.hidden = true;
      btn.setAttribute('aria-expanded', 'false');
      document.removeEventListener('click', outside, true);
      document.removeEventListener('keydown', onEsc, true);
    }
    function outside(e) {
      if (!wrap.contains(e.target)) close();
    }
    function onEsc(e) {
      if (e.key === 'Escape' || e.keyCode === 27) {
        close();
        btn.focus();
      }
    }
    btn.addEventListener('click', function () {
      if (panel.hidden) open();
      else close();
    });

    wrap.appendChild(btn);
    wrap.appendChild(panel);
    bar.appendChild(wrap);

    V.updaters.push(function () {
      var sel = V.state.facets[col.key] || [];
      boxes.forEach(function (b) {
        b.checked = sel.indexOf(b.value) >= 0;
      });
      refreshBadge();
    });
  }

  function buildBoolean(V, bar, col) {
    var wrap = h('div', 'docgen-base-boolean docgen-base-field');
    var lbl = h('label', 'docgen-base-field__label');
    lbl.textContent = col.header;
    var sel = h('select', null);
    [
      ['', 'Any'],
      ['true', 'True'],
      ['false', 'False']
    ].forEach(function (o) {
      var opt = document.createElement('option');
      opt.value = o[0];
      opt.textContent = o[1];
      sel.appendChild(opt);
    });
    lbl.appendChild(sel);
    wrap.appendChild(lbl);
    bar.appendChild(wrap);
    sel.addEventListener('change', function () {
      if (sel.value) V.state.facets[col.key] = [sel.value];
      else delete V.state.facets[col.key];
      V.state.page = 0;
      applyAndRender(V);
    });
    V.updaters.push(function () {
      var cur = V.state.facets[col.key];
      sel.value = cur && cur.length ? cur[0] : '';
    });
  }

  function buildDateRange(V, bar, col) {
    var wrap = h('div', 'docgen-base-daterange docgen-base-field');
    var legend = h('span', 'docgen-base-field__label');
    legend.textContent = col.header;
    wrap.appendChild(legend);
    var from = h('input', 'docgen-base-range__from', {
      type: 'date',
      'aria-label': col.header + ' from'
    });
    var to = h('input', 'docgen-base-range__to', {
      type: 'date',
      'aria-label': col.header + ' to'
    });
    var sep = h('span', 'docgen-base-range__sep', { 'aria-hidden': 'true' });
    sep.textContent = '–';
    wrap.appendChild(from);
    wrap.appendChild(sep);
    wrap.appendChild(to);
    bar.appendChild(wrap);
    function onChange() {
      var r = {};
      var f = dateInputToEpoch(from.value, false);
      var t = dateInputToEpoch(to.value, true);
      if (f != null) r.from = f;
      if (t != null) r.to = t;
      if (r.from != null || r.to != null) V.state.dates[col.key] = r;
      else delete V.state.dates[col.key];
      V.state.page = 0;
      applyAndRender(V);
    }
    from.addEventListener('change', onChange);
    to.addEventListener('change', onChange);
    V.updaters.push(function () {
      var r = V.state.dates[col.key] || {};
      from.value = r.from != null ? epochToDateInput(r.from) : '';
      to.value = r.to != null ? epochToDateInput(r.to) : '';
    });
  }

  function buildNumberRange(V, bar, col) {
    var wrap = h('div', 'docgen-base-numrange docgen-base-field');
    var legend = h('span', 'docgen-base-field__label');
    legend.textContent = col.header;
    wrap.appendChild(legend);
    var from = h('input', 'docgen-base-range__from', {
      type: 'number',
      step: 'any',
      placeholder: 'min',
      'aria-label': col.header + ' min'
    });
    var to = h('input', 'docgen-base-range__to', {
      type: 'number',
      step: 'any',
      placeholder: 'max',
      'aria-label': col.header + ' max'
    });
    var sep = h('span', 'docgen-base-range__sep', { 'aria-hidden': 'true' });
    sep.textContent = '–';
    wrap.appendChild(from);
    wrap.appendChild(sep);
    wrap.appendChild(to);
    bar.appendChild(wrap);
    function onChange() {
      var r = {};
      if (from.value !== '') r.from = Number(from.value);
      if (to.value !== '') r.to = Number(to.value);
      if (r.from != null || r.to != null) V.state.numbers[col.key] = r;
      else delete V.state.numbers[col.key];
      V.state.page = 0;
      applyAndRender(V);
    }
    from.addEventListener('input', onChange);
    to.addEventListener('input', onChange);
    V.updaters.push(function () {
      var r = V.state.numbers[col.key] || {};
      from.value = r.from != null ? r.from : '';
      to.value = r.to != null ? r.to : '';
    });
  }

  function buildSortSelect(V, bar) {
    var wrap = h('div', 'docgen-base-field');
    var lbl = h('label', 'docgen-base-field__label');
    lbl.textContent = 'Sort';
    var sel = h('select', 'docgen-base-sort');
    var def = document.createElement('option');
    def.value = '';
    def.textContent = 'Default';
    sel.appendChild(def);
    V.columns.forEach(function (col) {
      if (!col.sortable) return;
      [
        [':asc', ' (ascending)'],
        [':desc', ' (descending)']
      ].forEach(function (o) {
        var opt = document.createElement('option');
        opt.value = col.key + o[0];
        opt.textContent = col.header + o[1];
        sel.appendChild(opt);
      });
    });
    lbl.appendChild(sel);
    wrap.appendChild(lbl);
    bar.appendChild(wrap);
    sel.addEventListener('change', function () {
      if (!sel.value) V.state.sort = [];
      else {
        var c = sel.value.split(':');
        V.state.sort = [{ col: c[0], desc: c[1] === 'desc' }];
      }
      applyAndRender(V);
    });
    V.updaters.push(function () {
      var s = V.state.sort && V.state.sort.length ? V.state.sort[0] : null;
      sel.value = s ? s.col + ':' + (s.desc ? 'desc' : 'asc') : '';
    });
  }

  // Table header sort: wrap sortable <th> text in a button; click cycles
  // asc -> desc -> none. Only ONE active header sort (shift-click NOT implemented;
  // documented v1 limitation — use the payload's multi-key sort as the default only).
  function wireSort(V) {
    if (V.view.type !== 'table') return;
    var ths = V.section.querySelectorAll('th[data-col]');
    V.sortHeaders = [];
    for (var i = 0; i < ths.length; i++) {
      (function (th) {
        var key = th.getAttribute('data-col');
        var meta = V.colByKey[key];
        if (!meta || !meta.sortable) return;
        var text = th.textContent;
        th.textContent = '';
        var btn = h('button', 'docgen-base-sort-th', { type: 'button' });
        btn.textContent = text;
        // Direction caret as a real aria-hidden element (not CSS ::after, which
        // some screen readers speak on top of the authoritative aria-sort state).
        var caret = h('span', 'docgen-base-sort-caret', { 'aria-hidden': 'true' });
        btn.appendChild(caret);
        th.appendChild(btn);
        btn.addEventListener('click', function () {
          var cur =
            V.state.sort && V.state.sort.length && V.state.sort[0].col === key
              ? V.state.sort[0]
              : null;
          if (!cur) V.state.sort = [{ col: key, desc: false }];
          else if (!cur.desc) V.state.sort = [{ col: key, desc: true }];
          else V.state.sort = [];
          applyAndRender(V);
        });
        V.sortHeaders.push({ th: th, key: key, caret: caret });
      })(ths[i]);
    }
    updateAriaSort(V);
  }

  function updateAriaSort(V) {
    if (!V.sortHeaders) return;
    var active =
      V.state.sort && V.state.sort.length ? V.state.sort[0] : null;
    V.sortHeaders.forEach(function (sh) {
      var isActive = active && active.col === sh.key;
      if (isActive)
        sh.th.setAttribute('aria-sort', active.desc ? 'descending' : 'ascending');
      else sh.th.setAttribute('aria-sort', 'none');
      if (sh.caret) sh.caret.textContent = isActive ? (active.desc ? '↓' : '↑') : '↕';
    });
  }

  function syncControlsFromState(V) {
    for (var i = 0; i < V.updaters.length; i++) V.updaters[i]();
    updateAriaSort(V);
  }

  // A stable signature of a sort-key array, to detect when the DOM's row order
  // needs to actually change.
  function sortSig(keys) {
    return (keys || [])
      .map(function (k) {
        return k.col + (k.desc ? ':d' : ':a');
      })
      .join(',');
  }

  function applyAndRender(V) {
    var i, id;
    // 1. matched id list (payload row order preserved => SSR order)
    var matched = [];
    for (i = 0; i < V.rows.length; i++) {
      if (matchRow(V.rows[i], V.state, V.columns)) matched.push(V.rows[i].id);
    }
    // 2. sort (skip if grouped — preserve SSR group order)
    var ordered = V.grouped
      ? matched
      : sortIds(matched, V.rowsById, V.state.sort);

    // 3. paginate
    var pageSize = (V.data.controls && V.data.controls.pageSize) || 0;
    var total = ordered.length;
    var pageCount = pageSize > 0 ? Math.max(1, Math.ceil(total / pageSize)) : 1;
    if (V.state.page < 0) V.state.page = 0;
    if (V.state.page > pageCount - 1) V.state.page = pageCount - 1;
    var windowIds;
    if (pageSize > 0) {
      var start = V.state.page * pageSize;
      windowIds = ordered.slice(start, start + pageSize);
    } else {
      windowIds = ordered;
    }
    var visibleSet = {};
    for (i = 0; i < windowIds.length; i++) visibleSet[windowIds[i]] = true;

    // 4a. Reorder (ungrouped only) ONLY when the sort actually changed. Filtering
    // and paging never change relative order, so re-appending every node on each
    // keystroke is pure waste (and forces reflow). Reorder the FULL row set (not
    // just matched) so hidden rows stay correctly positioned for later reveal.
    if (!V.grouped) {
      var sig = sortSig(V.state.sort);
      if (sig !== V.domOrderSig) {
        var fullOrder = sortIds(V.allIds, V.rowsById, V.state.sort);
        for (i = 0; i < fullOrder.length; i++) {
          var node = V.nodeById[fullOrder[i]];
          if (node) V.container.appendChild(node);
        }
        V.domOrderSig = sig;
      }
    }
    // 4b. Visibility: only toggle nodes whose visibility actually changed since the
    // last render (delta), so per-keystroke DOM writes are proportional to the
    // change, not to the total row count.
    var prevVisible = V.prevVisible || {};
    for (id in V.nodeById) {
      if (!Object.prototype.hasOwnProperty.call(V.nodeById, id)) continue;
      var want = !!visibleSet[id];
      if (want === !!prevVisible[id]) continue;
      var n = V.nodeById[id];
      if (want) n.removeAttribute('hidden');
      else n.setAttribute('hidden', '');
    }
    V.prevVisible = visibleSet;
    // 4c. Table groups: hide a group header with no visible data row in its group.
    if (V.view.type === 'table' && V.grouped) hideEmptyGroups(V);

    // 4d. Empty state.
    toggleEmpty(V, total === 0);

    // 5. status + pager + url
    if (V.statusEl)
      V.statusEl.textContent = matched.length + ' of ' + V.rows.length + ' results';
    updatePager(V, total, pageSize);
    updateAriaSort(V);
    syncUrl(V);
  }

  function hideEmptyGroups(V) {
    var groups = V.container.querySelectorAll('tr.docgen-base-group');
    for (var g = 0; g < groups.length; g++) {
      var gr = groups[g];
      var anyVisible = false;
      var sib = gr.nextElementSibling;
      while (sib && !sib.classList.contains('docgen-base-group')) {
        if (sib.hasAttribute('data-row') && !sib.hasAttribute('hidden')) {
          anyVisible = true;
          break;
        }
        sib = sib.nextElementSibling;
      }
      if (anyVisible) gr.removeAttribute('hidden');
      else gr.setAttribute('hidden', '');
    }
  }

  function toggleEmpty(V, show) {
    if (!V.emptyNode) {
      // Reuse an existing SSR empty marker if present, else inject one.
      var existing = V.section.querySelector('.docgen-base-empty');
      if (existing) {
        // wrap-aware: for the table the SSR marker is a <td> inside a <tr>.
        V.emptyNode =
          V.view.type === 'table' ? existing.parentNode || existing : existing;
      } else {
        V.emptyNode = createEmpty(V);
      }
    }
    if (show) V.emptyNode.removeAttribute('hidden');
    else V.emptyNode.setAttribute('hidden', '');
  }

  function createEmpty(V) {
    if (V.view.type === 'table') {
      var tr = h('tr', 'docgen-base-empty-row');
      var td = h('td', 'docgen-base-empty', {
        colspan: String(Math.max(1, V.columns.length))
      });
      td.textContent = 'No results';
      tr.appendChild(td);
      V.container.appendChild(tr);
      return tr;
    }
    if (V.view.type === 'list') {
      var li = h('li', 'docgen-base-empty');
      li.textContent = 'No results';
      V.container.appendChild(li);
      return li;
    }
    var div = h('div', 'docgen-base-empty');
    div.textContent = 'No results';
    V.container.appendChild(div);
    return div;
  }

  function updatePager(V, total, pageSize) {
    if (!V.pager) return;
    if (pageSize <= 0) {
      V.pager.el.hidden = true;
      return;
    }
    V.pager.el.hidden = false;
    var pageCount = Math.max(1, Math.ceil(total / pageSize));
    var start = total === 0 ? 0 : V.state.page * pageSize + 1;
    var end = Math.min(total, (V.state.page + 1) * pageSize);
    V.pager.label.textContent = start + '–' + end + ' of ' + total;
    // Disabling the button that currently holds focus would drop focus to <body>
    // (lost place, no visible indicator). Move focus to the still-enabled sibling
    // first. Set prev before next so reaching the last page lands focus on prev.
    setDisabled(V.pager.prev, V.state.page <= 0, V.pager.next);
    setDisabled(V.pager.next, V.state.page >= pageCount - 1, V.pager.prev);
  }

  function setDisabled(btn, disabled, sibling) {
    if (
      disabled &&
      document.activeElement === btn &&
      sibling &&
      !sibling.disabled
    ) {
      try {
        sibling.focus();
      } catch (e) {
        /* ignore */
      }
    }
    btn.disabled = disabled;
  }

  // --- URL sync -------------------------------------------------------------
  function syncUrl(V) {
    var encoded = encodeState(V.state);
    var prefix = 'b' + V.idx + '.';
    var mine = encoded
      ? encoded.split('&').map(function (p) {
          return prefix + p;
        })
      : [];
    var hash = location.hash.replace(/^#/, '');
    var others = hash
      ? hash.split('&').filter(function (p) {
          return p && p.indexOf(prefix) !== 0;
        })
      : [];
    var all = others.concat(mine);
    var newHash = all.join('&');
    var url =
      location.pathname + location.search + (newHash ? '#' + newHash : '');
    // replaceState (per product decision): shareable + reload, no history spam.
    try {
      history.replaceState(null, '', url);
    } catch (e) {
      /* file:// or sandboxed — ignore */
    }
  }

  function restoreFromUrl(V) {
    var prefix = 'b' + V.idx + '.';
    var hash = location.hash.replace(/^#/, '');
    var segs = hash
      ? hash
          .split('&')
          .filter(function (p) {
            return p.indexOf(prefix) === 0;
          })
          .map(function (p) {
            return p.slice(prefix.length);
          })
      : [];
    // No segments for this view (empty hash, or the hash was cleared/edited to
    // drop this view) => reset to defaults. Returning early here would leave
    // stale filter/sort state visible while the URL carries none.
    V.state = segs.length ? decodeState(segs.join('&')) : makeState(V.data);
  }

  function onHashChange() {
    for (var i = 0; i < VIEWS.length; i++) {
      var V = VIEWS[i];
      var before = encodeState(V.state);
      restoreFromUrl(V);
      // Only re-render if this view's slice actually changed.
      if (encodeState(V.state) !== before) {
        syncControlsFromState(V);
        applyAndRender(V);
      }
    }
  }

  // small helper: id-safe token from a column key
  function i2id(key) {
    return String(key).replace(/[^a-zA-Z0-9_-]/g, '_');
  }

  // ==========================================================================
  // ===== INIT ===============================================================
  // ==========================================================================
  function init() {
    var views = document.querySelectorAll('.docgen-base-view[data-base-view]');
    for (var i = 0; i < views.length; i++) hydrate(views[i]);
    if (VIEWS.length) {
      window.addEventListener('hashchange', onHashChange);
      window.addEventListener('popstate', onHashChange);
    }
  }

  // Only touch the DOM in a browser — under Node (test harness) `document` is
  // undefined and we just export the pure functions below.
  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', init);
    } else {
      init();
    }
    if (typeof window !== 'undefined' && window.docgen && window.docgen.island) {
      window.docgen.island('docgenBase', function () {});
    }
  }

  // Node test harness only (invisible in the browser: `module` is undefined).
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = {
      isNull: isNull,
      cmpNum: cmpNum,
      cmpStr: cmpStr,
      compareCells: compareCells,
      sortIds: sortIds,
      facetTokens: facetTokens,
      deriveFacets: deriveFacets,
      rowHaystack: rowHaystack,
      matchSearch: matchSearch,
      matchRow: matchRow,
      encodeState: encodeState,
      decodeState: decodeState
    };
  }
})();
