// docgen interactive-bases island — M3 STUB.
//
// This is the milestone-3 wiring stub: it proves the asset ships, loads, finds
// each interactive base view, and parses its embedded JSON payload — but adds NO
// controls. M4 fills in sort/filter/search/pagination behavior against the
// already-hydrated SSR DOM (see .overnight/interactive-bases/SCHEMA.md).
//
// The emitter (docgen-bases render.rs) wraps each interactive view as
//   <section class="docgen-base-view" data-base-view="i">
//     <script type="application/json" class="docgen-base-data">{...}</script>
//     ...static SSR rows...
//   </section>
// On parse failure we leave the static HTML untouched (progressive enhancement).
//
// Runs at DOM-ready; registers a no-op island to join the bootstrap registry.
(function () {
  function init() {
    var views = document.querySelectorAll('.docgen-base-view[data-base-view]');
    var errored = false;
    for (var i = 0; i < views.length; i++) {
      var section = views[i];
      var tag = section.querySelector('script.docgen-base-data');
      if (!tag) continue;
      try {
        var data = JSON.parse(tag.textContent);
        section.dataset.baseHydrated = '1';
        console.debug(
          '[docgen bases] payload v' + data.v + ' rows=' + data.rows.length
        );
      } catch (e) {
        // Leave the static HTML as-is (no-JS parity); report once.
        if (!errored) {
          errored = true;
          console.error('[docgen bases] failed to parse payload', e);
        }
      }
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  if (window.docgen && window.docgen.island) {
    window.docgen.island('docgenBase', function () {});
  }
})();
