/* docgen search: Cmd/Ctrl-K modal over /search-index.json. No deps, no npm. */
(function () {
  "use strict";
  var index = null;
  var loading = null;

  function loadIndex() {
    if (index) return Promise.resolve(index);
    if (loading) return loading;
    loading = fetch("/search-index.json")
      .then(function (r) { return r.json(); })
      .then(function (data) { index = data; return index; });
    return loading;
  }

  // Substring score; lower is better. -1 means no match.
  function score(entry, q) {
    var hay = (entry.title + " " + entry.text).toLowerCase();
    var i = hay.indexOf(q);
    if (i === -1) return -1;
    // Prefer title hits and earlier positions.
    var titleHit = entry.title.toLowerCase().indexOf(q) !== -1 ? 0 : 1000;
    return titleHit + i;
  }

  function search(q) {
    q = q.trim().toLowerCase();
    if (!q || !index) return [];
    return index
      .map(function (e) { return { e: e, s: score(e, q) }; })
      .filter(function (x) { return x.s >= 0; })
      .sort(function (a, b) { return a.s - b.s; })
      .slice(0, 20)
      .map(function (x) { return x.e; });
  }

  var modal, input, list, selected = 0, results = [];

  function buildModal() {
    modal = document.createElement("div");
    modal.className = "docgen-search-modal";
    modal.setAttribute("hidden", "");
    modal.innerHTML =
      '<div class="docgen-search-backdrop" data-close></div>' +
      '<div class="docgen-search-box" role="dialog" aria-modal="true" aria-label="Search">' +
      '<input class="docgen-search-input" type="text" placeholder="Search docs..." aria-label="Search docs" />' +
      '<ul class="docgen-search-results"></ul></div>';
    document.body.appendChild(modal);
    input = modal.querySelector(".docgen-search-input");
    list = modal.querySelector(".docgen-search-results");

    input.addEventListener("input", function () { render(search(input.value)); });
    input.addEventListener("keydown", onKey);
    modal.addEventListener("click", function (ev) {
      if (ev.target.hasAttribute("data-close")) close();
    });
  }

  function render(rs) {
    results = rs; selected = 0;
    list.innerHTML = "";
    rs.forEach(function (e, i) {
      var li = document.createElement("li");
      li.className = "docgen-search-result" + (i === 0 ? " is-selected" : "");
      // Build the row with DOM APIs so a slug containing `"`, `<` or `>`
      // (legal in file names) cannot break out of the attribute or inject markup.
      var a = document.createElement("a");
      a.setAttribute("href", "/" + e.slug);
      var span = document.createElement("span");
      span.className = "title";
      span.textContent = e.title;
      a.appendChild(span);
      li.appendChild(a);
      li.addEventListener("mouseenter", function () { select(i); });
      list.appendChild(li);
    });
  }

  function select(i) {
    if (!results.length) return;
    selected = (i + results.length) % results.length;
    var items = list.querySelectorAll(".docgen-search-result");
    items.forEach(function (el, idx) { el.classList.toggle("is-selected", idx === selected); });
  }

  function go() {
    if (results[selected]) window.location.href = "/" + results[selected].slug;
  }

  function onKey(ev) {
    if (ev.key === "ArrowDown") { ev.preventDefault(); select(selected + 1); }
    else if (ev.key === "ArrowUp") { ev.preventDefault(); select(selected - 1); }
    else if (ev.key === "Enter") { ev.preventDefault(); go(); }
    else if (ev.key === "Escape") { close(); }
  }

  function open() {
    if (!modal) buildModal();
    loadIndex().then(function () { render(search(input.value)); });
    modal.removeAttribute("hidden");
    input.value = ""; list.innerHTML = "";
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
