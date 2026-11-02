// Topbar preflight — the controls sibling of theme-preflight.js.
//
// Included by base.html as a BLOCKING <script src> right after the topbar
// markup: <body> (the .docgen-app element) and every dock button exist by
// then, and nothing below has painted yet, so the stored layout classes and
// pressed states are on the first painted frame. The layout island
// (islands/layout.js) otherwise restores these at DOM-ready — after first
// paint — which popped the pressed styles in and reflowed the page (content
// column widening, rail disappearing) on every navigation for users with
// stored prefs.
//
// Keys MUST match islands/layout.js:
//   localStorage  'doc-full-width'           'true'/'false', default false
//   localStorage  'doc-right-rail-collapsed' 'true'/'false', default false
// init() in the island re-applies both at DOM-ready (no-ops by then) and owns
// the live wiring. External file, not inline, for the CSP pin asserted by
// docgen-render's no_template_emits_inline_scripts test.
(function () {
  var body = document.body;
  if (!body) return;
  function readBool(key, fallback) {
    try {
      var raw = localStorage.getItem(key);
      if (raw === 'true') return true;
      if (raw === 'false') return false;
    } catch (e) {}
    return fallback;
  }
  try {
    var fullWidth = readBool('doc-full-width', false);
    body.classList.toggle('is-full-width', fullWidth);
    var fwBtn = document.querySelector('.docgen-ctl--fullwidth');
    if (fwBtn) fwBtn.setAttribute('aria-pressed', fullWidth ? 'true' : 'false');

    // Persisted flag is "collapsed"; pressed = rail VISIBLE (not collapsed).
    var collapsed = readBool('doc-right-rail-collapsed', false);
    body.classList.toggle('is-rail-collapsed', collapsed);
    var railBtn = document.querySelector('.docgen-ctl--rail');
    if (railBtn) railBtn.setAttribute('aria-pressed', collapsed ? 'false' : 'true');
  } catch (e) {}
})();
