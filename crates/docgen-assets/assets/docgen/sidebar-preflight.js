// docgen sidebar preflight — the tree sibling of theme-preflight.js.
//
// The templates load this file with a blocking <script src> immediately after
// the sidebar markup, so it executes synchronously BEFORE the first paint of
// anything below it. It applies the two persisted tree prefs that the layout
// island would otherwise only restore at DOM-ready — which lands after first
// paint and made every navigation flash the default tree (folders open,
// scroll at top) and animate the chevrons into place.
//
// Keys MUST match islands/layout.js:
//   localStorage  'doc-sidebar-collapsed'  JSON array of collapsed folder paths
//   sessionStorage 'doc-sidebar-scroll'    sidebar scrollTop in px
// init() in the island re-applies both at DOM-ready (a no-op by then) and owns
// all the live wiring: toggle persistence, scroll saving, resizer.
//
// External file, not inline, so a strict `script-src 'self'` CSP keeps
// blocking every page; the pin is asserted by docgen-render's
// no_template_emits_inline_scripts test.
(function () {
  var sidebar = document.getElementById('docgen-sidebar');
  if (!sidebar) return;
  try {
    var arr = JSON.parse(localStorage.getItem('doc-sidebar-collapsed') || '[]');
    var collapsed = {};
    if (Array.isArray(arr)) arr.forEach(function (p) { collapsed[p] = 1; });
    document.querySelectorAll('.docgen-tree__details[data-tree-path]').forEach(function (d) {
      if (collapsed[d.getAttribute('data-tree-path')]) d.open = false;
    });
  } catch (e) {}
  try {
    var top = parseInt(sessionStorage.getItem('doc-sidebar-scroll'), 10);
    if (isFinite(top)) {
      var max = sidebar.scrollHeight - sidebar.clientHeight;
      sidebar.scrollTop = max > 0 ? Math.min(top, max) : 0;
    }
  } catch (e) {}
})();
