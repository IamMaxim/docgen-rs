// docgen layout island. Owns the two topbar layout toggles that the Svelte
// site exposes via ui-prefs: full-width content and right-rail visibility.
//
// Unlike the theme toggle, the layout buttons (`.docgen-ctl--fullwidth` /
// `.docgen-ctl--rail`) are plain <button>s in `page.html` — NOT Alpine x-data.
// This island is their canonical controller: it reads localStorage at boot,
// applies the state classes to `.docgen-app`, wires clicks, persists, and keeps
// each button's `aria-pressed` in sync. It therefore runs at DOM-ready rather
// than waiting for `alpine:init`. A no-op island is still registered so the
// file participates in the bootstrap registry like its siblings.
//
// localStorage keys (match ui-prefs.ts):
//   doc-full-width            "true"/"false"  default false
//   doc-right-rail-collapsed  "true"/"false"  default false (rail VISIBLE)
// State classes on `.docgen-app`:
//   .is-full-width        content drops its max-width
//   .is-rail-collapsed    right rail hidden + layout track collapsed
(function () {
  var FULL_KEY = 'doc-full-width';
  var RAIL_KEY = 'doc-right-rail-collapsed';

  function readBool(key, fallback) {
    try {
      var raw = localStorage.getItem(key);
      if (raw === 'true') return true;
      if (raw === 'false') return false;
    } catch (e) {}
    return fallback;
  }

  function writeBool(key, value) {
    try {
      localStorage.setItem(key, value ? 'true' : 'false');
    } catch (e) {}
  }

  function init() {
    var app = document.querySelector('.docgen-app');
    if (!app) return;

    var fullWidth = readBool(FULL_KEY, false);
    // Persisted flag is "collapsed"; default false => rail visible.
    var railCollapsed = readBool(RAIL_KEY, false);

    var fullBtn = document.querySelector('.docgen-ctl--fullwidth');
    var railBtn = document.querySelector('.docgen-ctl--rail');

    function applyFullWidth() {
      app.classList.toggle('is-full-width', fullWidth);
      // pressed = full-width ON.
      if (fullBtn) fullBtn.setAttribute('aria-pressed', fullWidth ? 'true' : 'false');
    }

    function applyRail() {
      app.classList.toggle('is-rail-collapsed', railCollapsed);
      // pressed = rail VISIBLE (i.e. NOT collapsed).
      if (railBtn) railBtn.setAttribute('aria-pressed', railCollapsed ? 'false' : 'true');
    }

    applyFullWidth();
    applyRail();

    if (fullBtn) {
      fullBtn.addEventListener('click', function () {
        fullWidth = !fullWidth;
        writeBool(FULL_KEY, fullWidth);
        applyFullWidth();
      });
    }

    if (railBtn) {
      railBtn.addEventListener('click', function () {
        railCollapsed = !railCollapsed;
        writeBool(RAIL_KEY, railCollapsed);
        applyRail();
      });
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // Participate in the registry so the bootstrap loop is uniform; the actual
  // work already happened at DOM-ready above.
  if (window.docgen && window.docgen.island) {
    window.docgen.island('docgenLayout', function () {});
  }
})();
