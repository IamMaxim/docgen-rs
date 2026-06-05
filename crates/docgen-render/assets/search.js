/* docgen search: Cmd/Ctrl-K modal over /search-index.json. No deps, no npm. */
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
  // Element focused before the modal opened, restored on close (WCAG 2.4.3).
  var opener = null;
  // App-shell siblings hidden from AT / removed from tab order while the modal
  // is open (focus trap + no obscured-content navigation; WCAG 2.1.2/2.4.3).
  var inertEls = [];

  function setShellInert(on) {
    inertEls = [];
    var kids = document.body.children;
    for (var i = 0; i < kids.length; i++) {
      var el = kids[i];
      if (el === modal || el.tagName === "SCRIPT") continue;
      if (on) {
        el.setAttribute("aria-hidden", "true");
        el.setAttribute("inert", "");
        inertEls.push(el);
      }
    }
    if (!on) {
      var all = document.body.children;
      for (var j = 0; j < all.length; j++) {
        all[j].removeAttribute("aria-hidden");
        all[j].removeAttribute("inert");
      }
    }
  }

  function focusables() {
    return Array.prototype.slice.call(
      modal.querySelectorAll('input, a[href], button:not([disabled])')
    ).filter(function (el) { return el.offsetParent !== null || el === input; });
  }

  function trapTab(ev) {
    if (ev.key !== "Tab") return;
    var f = focusables();
    if (!f.length) { ev.preventDefault(); return; }
    var first = f[0], last = f[f.length - 1];
    var active = document.activeElement;
    if (ev.shiftKey && active === first) { ev.preventDefault(); last.focus(); }
    else if (!ev.shiftKey && active === last) { ev.preventDefault(); first.focus(); }
  }

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
    // Escape + Tab-trap work no matter where focus sits inside the dialog.
    modal.addEventListener("keydown", function (ev) {
      if (ev.key === "Escape") { ev.preventDefault(); close(); return; }
      trapTab(ev);
    });
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
      a.setAttribute("href", BASE + "/" + e.slug);
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
    if (!modal.hasAttribute("hidden")) return;
    opener = document.activeElement;
    loadIndex().then(function () { render(search(input.value)); });
    modal.removeAttribute("hidden");
    setShellInert(true);
    input.value = ""; list.innerHTML = "";
    input.focus();
  }
  function close() {
    if (!modal || modal.hasAttribute("hidden")) return;
    modal.setAttribute("hidden", "");
    setShellInert(false);
    if (opener && typeof opener.focus === "function") opener.focus();
    opener = null;
  }

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
