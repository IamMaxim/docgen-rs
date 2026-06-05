// docgen island registry. Islands push a registrar; bootstrap runs them all,
// then Alpine starts exactly once. Lazy libs (mermaid, codemirror) are fetched
// by the island itself inside its registrar/x-init, only when present on the page.
(function () {
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
