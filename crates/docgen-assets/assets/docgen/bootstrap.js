// docgen island registry. Islands push a registrar; bootstrap runs them all,
// then Alpine starts exactly once. Lazy libs (e.g. mermaid) are fetched
// by the island itself inside its registrar/x-init, only when present on the page.
(function () {
  // Deploy base for islands/search. The templates carry it on
  // <html data-docgen-base="..."> instead of an inline script so emitted pages
  // run under a strict Content-Security-Policy (script-src 'self'). Everything
  // that reads window.DOCGEN_BASE executes after this script (islands load
  // later in <body>; search.js is deferred).
  window.DOCGEN_BASE = document.documentElement.getAttribute('data-docgen-base') || '';
  window.docgen = window.docgen || {};
  const islands = (window.docgen.islands = window.docgen.islands || []);

  /** Register an island. fn receives the Alpine global once Alpine is ready. */
  window.docgen.island = function (name, fn) {
    islands.push({ name, fn });
  };

  /** Lazy-load a script once; returns a cached promise. Used by lazy islands. */
  const loaded = {};
  window.docgen.loadScript = function (src) {
    if (loaded[src]) return loaded[src];
    loaded[src] = new Promise(function (res, rej) {
      const s = document.createElement('script');
      s.src = src;
      s.onload = res;
      s.onerror = rej;
      document.head.appendChild(s);
    });
    return loaded[src];
  };

  document.addEventListener('alpine:init', function () {
    for (const entry of islands) {
      try {
        entry.fn(window.Alpine);
      } catch (e) {
        console.error('[docgen island]', entry.name, e);
      }
    }
  });
})();
