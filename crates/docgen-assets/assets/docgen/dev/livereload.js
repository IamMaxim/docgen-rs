// Dev-only live reload. Connects to the dev server's SSE channel; on a `reload`
// event, reloads the page. Never emitted by `docgen build`.
//
// Critically, the EventSource is CLOSED on `pagehide` and reopened on `pageshow`.
// A long-lived SSE connection counts against the browser's per-origin HTTP/1.1
// connection cap (~6). When a page is frozen into the back/forward cache on
// navigation, its EventSource is NOT auto-closed — so rapidly clicking between
// pages would strand one connection per cached page until the pool is exhausted
// and new requests stall ("pool exhausted" stutter). Closing on pagehide frees
// the slot immediately; reopening on a bfcache restore (pageshow persisted)
// keeps live-reload working when the user navigates back.
(function () {
  var es = null;

  function connect() {
    if (es) return;
    try {
      es = new EventSource('/__docgen/livereload');
      es.addEventListener('reload', function () {
        location.reload();
      });
      es.onerror = function () {
        /* server restarting; EventSource auto-retries */
      };
    } catch (e) {
      console.warn('[docgen] livereload unavailable', e);
    }
  }

  function disconnect() {
    if (es) {
      es.close();
      es = null;
    }
  }

  connect();
  // Free the connection when the page is hidden (navigation away / bfcache).
  window.addEventListener('pagehide', disconnect);
  // Reconnect when restored from the back/forward cache.
  window.addEventListener('pageshow', function (e) {
    if (e.persisted) connect();
  });
})();
