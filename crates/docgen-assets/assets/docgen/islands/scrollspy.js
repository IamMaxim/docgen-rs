// docgen scroll-spy island for the right-rail "On this page" TOC.
//
// Mirrors RightRail.svelte: observes the in-content headings with an
// IntersectionObserver (rootMargin '-84px 0px -70% 0px') and marks the matching
// rail TOC link `.is-active`. The TOC is server-rendered by `page.html` (links
// carry `data-toc-id="<heading id>"`); if for some reason it is absent, this
// island builds it from the headings so the rail is never empty.
//
// Runs at DOM-ready (no Alpine dependency); also registers a no-op island so it
// joins the bootstrap registry like its siblings.
(function () {
  var ROOT_MARGIN = '-84px 0px -70% 0px';

  function headings() {
    var content = document.querySelector('.docgen-doc-content');
    if (!content) return [];
    return Array.prototype.slice.call(content.querySelectorAll('h2[id], h3[id]'));
  }

  function buildToc(nav, hs) {
    hs.forEach(function (h) {
      var a = document.createElement('a');
      a.className = 'docgen-rail__toc-link';
      if (h.tagName.toLowerCase() === 'h3') a.className += ' is-depth-3';
      a.href = '#' + h.id;
      a.setAttribute('data-toc-id', h.id);
      a.textContent = h.textContent || '';
      nav.appendChild(a);
    });
  }

  function init() {
    var hs = headings();
    if (!hs.length) return;

    var nav = document.querySelector('.docgen-rail__toc');
    // If the rail TOC wasn't server-rendered, build it (best effort — needs an
    // existing `.docgen-rail__toc` container; if none, scroll-spy is a no-op).
    if (nav && !nav.querySelector('.docgen-rail__toc-link')) {
      buildToc(nav, hs);
    }

    var links = {};
    Array.prototype.slice
      .call(document.querySelectorAll('.docgen-rail__toc-link'))
      .forEach(function (a) {
        var id = a.getAttribute('data-toc-id') || (a.getAttribute('href') || '').replace(/^#/, '');
        if (id) links[id] = a;
      });

    function setActive(id) {
      for (var key in links) {
        if (Object.prototype.hasOwnProperty.call(links, key)) {
          links[key].classList.toggle('is-active', key === id);
        }
      }
    }

    // Smooth-scroll + URL sync on click.
    Array.prototype.slice
      .call(document.querySelectorAll('.docgen-rail__toc-link'))
      .forEach(function (a) {
        a.addEventListener('click', function (e) {
          var id = a.getAttribute('data-toc-id') || (a.getAttribute('href') || '').replace(/^#/, '');
          var target = id && document.getElementById(id);
          if (!target) return;
          e.preventDefault();
          target.scrollIntoView({ behavior: 'smooth', block: 'start' });
          setActive(id);
          try {
            history.replaceState(null, '', '#' + id);
          } catch (err) {}
        });
      });

    if (!('IntersectionObserver' in window)) return;

    var visible = {};
    var observer = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) visible[entry.target.id] = true;
          else delete visible[entry.target.id];
        });
        // Pick the first heading (document order) currently in the band.
        for (var i = 0; i < hs.length; i++) {
          if (visible[hs[i].id]) {
            setActive(hs[i].id);
            return;
          }
        }
      },
      { rootMargin: ROOT_MARGIN, threshold: 0 }
    );

    hs.forEach(function (h) {
      observer.observe(h);
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  if (window.docgen && window.docgen.island) {
    window.docgen.island('docgenScrollspy', function () {});
  }
})();
