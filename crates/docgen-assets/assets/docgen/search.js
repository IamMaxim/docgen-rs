/* docgen search: Cmd/Ctrl-K modal over /search-index.json. No deps, no npm.
 *
 * Scoring + grouping port the Svelte `SearchModal.svelte` model: a tiered fuzzy
 * score (exact > prefix > substring > subsequence) feeds two result groups built
 * from the fields the Rust `SearchEntry` carries (slug/title/text):
 *   - "Pages"     — title/slug matches (the page itself), boosted.
 *   - "Full text" — body matches, with a context excerpt around the hit.
 * (The Svelte index additionally ships per-doc headings/callouts/rust-refs; the
 * Rust index does not, so those groups are omitted gracefully — see the build
 * notes. Enriching `SearchEntry` would touch the core pipeline + several crates,
 * so it is intentionally out of scope here.)
 *
 * Keyboard: ↑/↓ move, Enter opens, Esc closes, Cmd/Ctrl-K toggles. */
(function () {
  "use strict";
  var index = null;
  var loading = null;
  // Deployed sub-path prefix (e.g. "/docs"); "" when served at the origin root.
  var BASE = (typeof window !== "undefined" && window.DOCGEN_BASE) || "";

  function loadIndex() {
    if (index) return Promise.resolve(index);
    if (loading) return loading;
    loading = fetch(BASE + "/search-index.json")
      .then(function (r) { return r.json(); })
      .then(function (data) { index = data; return index; });
    return loading;
  }

  function normalize(value) {
    return (value || "").toLowerCase().replace(/\s+/g, " ").trim();
  }

  // Tiered fuzzy score; higher is better, 0 = no match. Mirrors the Svelte
  // `fuzzyScore`: exact 100, prefix 80, substring 60, subsequence 18.
  function fuzzyScore(haystack, needle) {
    if (!needle) return 1;
    var text = normalize(haystack);
    var term = normalize(needle);
    if (text === term) return 100;
    if (text.indexOf(term) === 0) return 80;
    if (text.indexOf(term) !== -1) return 60;
    var cursor = 0;
    for (var i = 0; i < term.length; i++) {
      var idx = text.indexOf(term[i], cursor);
      if (idx === -1) return 0;
      cursor = idx + 1;
    }
    return 18;
  }

  // A short window of body text around the first match, with ellipses.
  function excerpt(text, term) {
    var clean = (text || "").replace(/\s+/g, " ").trim();
    if (!clean) return "";
    var idx = clean.toLowerCase().indexOf((term || "").toLowerCase());
    if (idx < 0) return clean.slice(0, 150);
    var start = Math.max(0, idx - 48);
    var end = Math.min(clean.length, idx + term.length + 92);
    return (start > 0 ? "…" : "") + clean.slice(start, end) + (end < clean.length ? "…" : "");
  }

  // Build the flat, ranked result list. Each result is one row: { kind, title,
  // path, slug, context, score }. `kind` drives grouping ("page" | "body").
  function buildResults(term) {
    if (!index) return [];
    var q = (term || "").trim();
    if (!q) {
      // Empty query: show the first handful of pages as a starting point.
      return index.slice(0, 8).map(function (e) {
        return { kind: "page", title: e.title, path: e.slug, slug: e.slug, context: "", score: 1 };
      });
    }
    var out = [];
    for (var i = 0; i < index.length; i++) {
      var e = index[i];
      var titleScore = fuzzyScore(e.title + " " + e.slug, q);
      if (titleScore > 0) {
        out.push({
          kind: "page",
          title: e.title,
          path: e.slug,
          slug: e.slug,
          context: "",
          score: titleScore + 30,
        });
      }
      var bodyScore = fuzzyScore(e.text, q);
      if (bodyScore > 0) {
        out.push({
          kind: "body",
          title: e.title,
          path: "Full text / " + e.slug,
          slug: e.slug,
          context: excerpt(e.text, q),
          score: bodyScore,
        });
      }
    }
    out.sort(function (a, b) {
      return b.score - a.score || a.title.localeCompare(b.title);
    });
    return out.slice(0, 24);
  }

  // Partition the ranked rows into the labeled, ordered groups the UI renders.
  function groupResults(rows) {
    var groups = [
      { kind: "page", label: "Pages", items: [] },
      { kind: "body", label: "Full text", items: [] },
    ];
    rows.forEach(function (r) {
      var g = groups.filter(function (x) { return x.kind === r.kind; })[0];
      if (g) g.items.push(r);
    });
    return groups.filter(function (g) { return g.items.length > 0; });
  }

  var modal, input, list, selected = 0, results = [];

  function buildModal() {
    modal = document.createElement("div");
    modal.className = "docgen-search-modal";
    modal.setAttribute("hidden", "");
    modal.innerHTML =
      '<div class="docgen-search-backdrop" data-close></div>' +
      '<div class="docgen-search-box" role="dialog" aria-modal="true" aria-label="Search">' +
      '<input class="docgen-search-input" type="text" placeholder="Search pages, headings, Rust refs…" aria-label="Search docs" />' +
      '<ul class="docgen-search-results"></ul></div>';
    document.body.appendChild(modal);
    input = modal.querySelector(".docgen-search-input");
    list = modal.querySelector(".docgen-search-results");

    input.addEventListener("input", function () { render(buildResults(input.value)); });
    input.addEventListener("keydown", onKey);
    modal.addEventListener("click", function (ev) {
      if (ev.target.hasAttribute("data-close")) close();
    });
  }

  // Append the highlighted title (matched substring wrapped in <mark>) into `el`
  // using DOM APIs, so an arbitrary slug/title cannot inject markup.
  function appendHighlighted(el, text, term) {
    var t = (term || "").trim();
    var idx = t ? text.toLowerCase().indexOf(t.toLowerCase()) : -1;
    if (idx < 0) {
      el.appendChild(document.createTextNode(text));
      return;
    }
    el.appendChild(document.createTextNode(text.slice(0, idx)));
    var mark = document.createElement("mark");
    mark.textContent = text.slice(idx, idx + t.length);
    el.appendChild(mark);
    el.appendChild(document.createTextNode(text.slice(idx + t.length)));
  }

  function render(rs) {
    results = rs; selected = 0;
    list.innerHTML = "";
    if (!rs.length) {
      var empty = document.createElement("li");
      empty.className = "docgen-search-empty";
      empty.textContent = input.value.trim()
        ? "No results for “" + input.value.trim() + "”."
        : "Type to search.";
      list.appendChild(empty);
      return;
    }
    var term = input.value;
    var groups = groupResults(rs);
    var flatIndex = 0;
    groups.forEach(function (group) {
      var head = document.createElement("li");
      head.className = "docgen-search-group";
      head.textContent = group.label;
      list.appendChild(head);
      group.items.forEach(function (r) {
        var i = flatIndex++;
        var li = document.createElement("li");
        li.className = "docgen-search-result" + (i === 0 ? " is-selected" : "");
        li.setAttribute("data-index", String(i));
        var a = document.createElement("a");
        a.setAttribute("href", BASE + "/" + r.slug);
        var main = document.createElement("span");
        main.className = "docgen-search-result__main";
        var title = document.createElement("span");
        title.className = "title";
        appendHighlighted(title, r.title, term);
        main.appendChild(title);
        var path = document.createElement("span");
        path.className = "docgen-search-result__path";
        path.textContent = r.path;
        main.appendChild(path);
        if (r.context) {
          var ctx = document.createElement("span");
          ctx.className = "docgen-search-result__context";
          ctx.textContent = r.context;
          main.appendChild(ctx);
        }
        a.appendChild(main);
        li.appendChild(a);
        li.addEventListener("mouseenter", (function (n) {
          return function () { select(n); };
        })(i));
        list.appendChild(li);
      });
    });
  }

  function select(i) {
    if (!results.length) return;
    selected = (i + results.length) % results.length;
    var items = list.querySelectorAll(".docgen-search-result");
    items.forEach(function (el, idx) {
      var active = idx === selected;
      el.classList.toggle("is-selected", active);
      if (active) el.scrollIntoView({ block: "nearest" });
    });
  }

  function go() {
    if (results[selected]) window.location.href = BASE + "/" + results[selected].slug;
  }

  function onKey(ev) {
    if (ev.key === "ArrowDown") { ev.preventDefault(); select(selected + 1); }
    else if (ev.key === "ArrowUp") { ev.preventDefault(); select(selected - 1); }
    else if (ev.key === "Enter") { ev.preventDefault(); go(); }
    else if (ev.key === "Escape") { close(); }
  }

  function open() {
    if (!modal) buildModal();
    input.value = "";
    list.innerHTML = "";
    modal.removeAttribute("hidden");
    loadIndex().then(function () { render(buildResults(input.value)); });
    input.focus();
  }
  function close() { if (modal) modal.setAttribute("hidden", ""); }

  document.addEventListener("keydown", function (ev) {
    if ((ev.metaKey || ev.ctrlKey) && (ev.key === "k" || ev.key === "K")) {
      ev.preventDefault(); open();
    }
  });
  document.addEventListener("click", function (ev) {
    var t = ev.target.closest("[data-docgen-search]");
    if (t) { ev.preventDefault(); open(); }
  });
})();
