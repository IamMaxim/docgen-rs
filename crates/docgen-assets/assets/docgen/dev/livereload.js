// Dev-only live reload. Connects to the dev server's SSE channel; on a `reload`
// event, reloads the page. Never emitted by `docgen build`.
(function () {
  try {
    var es = new EventSource('/__docgen/livereload');
    es.addEventListener('reload', function () {
      location.reload();
    });
    es.onerror = function () {
      /* server restarting; EventSource auto-retries */
    };
  } catch (e) {
    console.warn('[docgen] livereload unavailable', e);
  }
})();
