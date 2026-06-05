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
// It also drives the draggable sidebar resize handle (`.docgen-rail-resizer`, a
// port of the original starter layout's `.rail-resizer`): pointer-drag updates
// `--left-rail-width` on <html> (clamped 180..560) and persists it so the
// sidebar width survives reloads. The pre-paint head script applies the stored
// width before first paint to avoid a layout jump.
//
// localStorage keys (match ui-prefs.ts):
//   doc-full-width            "true"/"false"  default false
//   doc-right-rail-collapsed  "true"/"false"  default false (rail VISIBLE)
//   doc-left-rail-width       int px, 180..560, default 264
// State classes on `.docgen-app`:
//   .is-full-width        content drops its max-width
//   .is-rail-collapsed    right rail hidden + layout track collapsed
(function () {
  var FULL_KEY = 'doc-full-width';
  var RAIL_KEY = 'doc-right-rail-collapsed';
  var WIDTH_KEY = 'doc-left-rail-width';
  var WIDTH_MIN = 180;
  var WIDTH_MAX = 560;
  var WIDTH_DEFAULT = 264;

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

    wireResizer();
  }

  function clampWidth(n) {
    if (!isFinite(n)) return WIDTH_DEFAULT;
    return Math.max(WIDTH_MIN, Math.min(WIDTH_MAX, Math.round(n)));
  }

  function readWidth() {
    try {
      var raw = parseInt(localStorage.getItem(WIDTH_KEY), 10);
      if (isFinite(raw)) return clampWidth(raw);
    } catch (e) {}
    return WIDTH_DEFAULT;
  }

  function setWidth(px) {
    document.documentElement.style.setProperty('--left-rail-width', px + 'px');
  }

  // Pointer-drag the `.docgen-rail-resizer` to resize the sidebar. Mirrors the
  // original `resizable` action: capture the pointer on down, translate X delta
  // into a clamped width on move, persist on up.
  function wireResizer() {
    var handle = document.querySelector('.docgen-rail-resizer');
    if (!handle) return;

    var width = readWidth();
    // Reconcile in case the pre-paint script missed (e.g. JS-disabled head).
    setWidth(width);

    var drag = null; // { pointerId, startX, startWidth }

    handle.addEventListener('pointerdown', function (e) {
      drag = { pointerId: e.pointerId, startX: e.clientX, startWidth: width };
      handle.classList.add('is-dragging');
      try {
        handle.setPointerCapture(e.pointerId);
      } catch (err) {}
      e.preventDefault();
    });

    handle.addEventListener('pointermove', function (e) {
      if (!drag || drag.pointerId !== e.pointerId) return;
      width = clampWidth(drag.startWidth + (e.clientX - drag.startX));
      setWidth(width);
    });

    function endDrag(e) {
      if (!drag || drag.pointerId !== e.pointerId) return;
      try {
        handle.releasePointerCapture(drag.pointerId);
      } catch (err) {}
      drag = null;
      handle.classList.remove('is-dragging');
      try {
        localStorage.setItem(WIDTH_KEY, String(width));
      } catch (err) {}
    }

    handle.addEventListener('pointerup', endDrag);
    handle.addEventListener('pointercancel', endDrag);
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
