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
// It also persists the sidebar folder tree's collapse state: each folder
// `<details data-tree-path>` open/closed state is stored so the tree looks the
// same after a reload or navigation (folders default open server-side).
//
// localStorage keys (match ui-prefs.ts):
//   doc-full-width            "true"/"false"  default false
//   doc-right-rail-collapsed  "true"/"false"  default false (rail VISIBLE)
//   doc-left-rail-width       int px, 180..560, default 264
//   doc-sidebar-collapsed     JSON array of collapsed folder paths, default []
// sessionStorage keys (transient, per-tab):
//   doc-sidebar-scroll        sidebar scrollTop in px, restored across nav
// State classes on `.docgen-app`:
//   .is-full-width        content drops its max-width
//   .is-rail-collapsed    right rail hidden + layout track collapsed
(function () {
  var FULL_KEY = 'doc-full-width';
  var RAIL_KEY = 'doc-right-rail-collapsed';
  var WIDTH_KEY = 'doc-left-rail-width';
  var TREE_KEY = 'doc-sidebar-collapsed';
  // sessionStorage (per-tab, transient) — sidebar scroll offset in px.
  var SCROLL_KEY = 'doc-sidebar-scroll';
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
        // In compact (≤1100px) the rail is an off-canvas drawer driven by Alpine
        // (`railOpen`); this desktop persisted-collapse toggle must not fire there
        // or it would fight the drawer state. The button is also hidden by CSS in
        // compact, but guard the handler too for safety.
        if (window.matchMedia('(max-width: 1100px)').matches) return;
        railCollapsed = !railCollapsed;
        writeBool(RAIL_KEY, railCollapsed);
        applyRail();
      });
    }

    wireResizer();
    wireTreeCollapse();
    wireSidebarScroll();
  }

  // Preserve the sidebar's scroll offset across the full-page reloads that every
  // nav link triggers. Without this the `#docgen-sidebar` scroll container (its
  // own `overflow-y:auto` element) is rebuilt on each navigation and snaps back
  // to the top — losing your place in a long tree. We stash `scrollTop` in
  // sessionStorage (transient, per-tab: it should not leak across browser
  // sessions or into unrelated tabs the way the width/collapse prefs do) and
  // restore it on the next load. Restore runs after `wireTreeCollapse` so the
  // stored offset is measured against the same (collapsed) tree height.
  function wireSidebarScroll() {
    var sidebar = document.getElementById('docgen-sidebar');
    if (!sidebar) return;

    // Restore first, clamped to the current scrollable range (the tree height can
    // differ between pages, e.g. the active branch expanded elsewhere).
    var stored = parseInt(sessionStorage.getItem(SCROLL_KEY), 10);
    if (isFinite(stored)) {
      var max = sidebar.scrollHeight - sidebar.clientHeight;
      sidebar.scrollTop = max > 0 ? Math.min(stored, max) : 0;
    }

    // Persist as the user scrolls (throttled to animation frames) and once more
    // on the way out, so a click that navigates captures the latest offset even
    // if the last scroll frame hadn't flushed.
    var pending = false;
    sidebar.addEventListener(
      'scroll',
      function () {
        if (pending) return;
        pending = true;
        requestAnimationFrame(function () {
          pending = false;
          saveScroll(sidebar);
        });
      },
      { passive: true }
    );
    // `pagehide` fires on navigation (and is bfcache-friendly, unlike unload).
    window.addEventListener('pagehide', function () {
      saveScroll(sidebar);
    });
  }

  function saveScroll(sidebar) {
    try {
      sessionStorage.setItem(SCROLL_KEY, String(sidebar.scrollTop));
    } catch (e) {}
  }

  // Persist the sidebar folder tree's open/closed state. Folders render `open`
  // server-side; here we collapse any folder whose `data-tree-path` is stored,
  // and keep the stored set in sync as the user toggles `<details>` elements.
  function wireTreeCollapse() {
    var folders = document.querySelectorAll('.docgen-tree__details[data-tree-path]');
    if (!folders.length) return;

    var collapsed = readSet(TREE_KEY);

    folders.forEach(function (d) {
      var path = d.getAttribute('data-tree-path');
      if (collapsed[path]) d.open = false;
      d.addEventListener('toggle', function () {
        // Re-read so concurrent toggles in other tabs/handlers don't clobber.
        var set = readSet(TREE_KEY);
        if (d.open) delete set[path];
        else set[path] = 1;
        writeSet(TREE_KEY, set);
      });
    });
  }

  // Collapsed paths are stored as a JSON array; loaded as a {path: 1} map for
  // O(1) lookup. Malformed/absent storage yields an empty map.
  function readSet(key) {
    try {
      var arr = JSON.parse(localStorage.getItem(key) || '[]');
      var map = {};
      if (Array.isArray(arr)) arr.forEach(function (p) { map[p] = 1; });
      return map;
    } catch (e) {
      return {};
    }
  }

  function writeSet(key, map) {
    try {
      localStorage.setItem(key, JSON.stringify(Object.keys(map)));
    } catch (e) {}
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
