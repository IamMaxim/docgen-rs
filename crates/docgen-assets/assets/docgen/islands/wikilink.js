// docgen wikilink hover-preview island. Ports WikilinkTooltip.svelte.
//
// Resolved wikilinks are rendered by docgen-core as
//   <a class="docgen-wikilink" href="…" data-wikilink-title="…" data-wikilink-path="…">…</a>
// On hover over such a link this island shows a fixed-position popover
// (`.docgen-wikilink-tooltip`) with the page title + path, clamped to the
// viewport, and hides it on mouse-out / click / scroll-away.
//
// Delegated listeners on `document` (so it covers links anywhere in content),
// but only `a[data-wikilink-title]` triggers it — broken wikilinks (spans with
// no such attribute) degrade gracefully to no tooltip.
//
// Runs at DOM-ready; registers a no-op island to join the bootstrap registry.
(function () {
  var SELECTOR = 'a[data-wikilink-title]';

  function closestLink(node) {
    if (!node || !node.closest) return null;
    return node.closest(SELECTOR);
  }

  function init() {
    var tip = document.createElement('div');
    tip.className = 'docgen-wikilink-tooltip';
    tip.setAttribute('role', 'tooltip');
    var titleEl = document.createElement('div');
    titleEl.className = 'docgen-wikilink-tooltip__title';
    var pathEl = document.createElement('div');
    pathEl.className = 'docgen-wikilink-tooltip__path';
    tip.appendChild(titleEl);
    tip.appendChild(pathEl);
    document.body.appendChild(tip);

    var current = null;

    function position(target) {
      var rect = target.getBoundingClientRect();
      var tipRect = tip.getBoundingClientRect();
      var tipW = tipRect.width || 240;
      var tipH = tipRect.height || 56;
      var x = rect.left + rect.width / 2 - tipW / 2;
      var y = rect.bottom + 6;
      if (y + tipH > window.innerHeight - 8) {
        y = rect.top - tipH - 6;
      }
      x = Math.max(8, Math.min(window.innerWidth - tipW - 8, x));
      tip.style.left = Math.round(x) + 'px';
      tip.style.top = Math.round(y) + 'px';
    }

    function show(link) {
      current = link;
      titleEl.textContent = link.getAttribute('data-wikilink-title') || '';
      pathEl.textContent = link.getAttribute('data-wikilink-path') || '';
      tip.classList.add('is-visible');
      // Measure after content is set, then place.
      requestAnimationFrame(function () {
        if (current === link) position(link);
      });
    }

    function hide() {
      current = null;
      tip.classList.remove('is-visible');
    }

    document.addEventListener('mouseover', function (e) {
      var link = closestLink(e.target);
      if (!link || link === current) return;
      show(link);
    });

    document.addEventListener('mouseout', function (e) {
      var link = closestLink(e.target);
      if (!link || link !== current) return;
      var next = closestLink(e.relatedTarget);
      if (next === link) return;
      hide();
    });

    document.addEventListener(
      'click',
      function (e) {
        if (closestLink(e.target)) hide();
      },
      true
    );

    window.addEventListener(
      'scroll',
      function () {
        if (current) position(current);
      },
      { passive: true, capture: true }
    );
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  if (window.docgen && window.docgen.island) {
    window.docgen.island('docgenWikilink', function () {});
  }
})();
