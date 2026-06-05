// Mermaid island: renders each .docgen-mermaid container by lazy-loading the
// vendored mermaid.min.js exactly once, only on pages where a container exists.
// Registered through the docgen.island convention (see bootstrap.js); classic
// script, no ESM imports (the no-npm discipline).
// Map the page's `data-theme` (set by the theme-toggle island on <html>) to a
// mermaid built-in theme: dark mode -> 'dark', anything else -> 'default'.
function docgenMermaidTheme() {
  try {
    return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'default';
  } catch (e) {
    return 'default';
  }
}

// Override mermaid's built-in theme variables with our design tokens so edge
// labels ("yes"/"no") sit on the diagram card surface instead of mermaid's
// default light pill — which read as bright chips against the dark card. We
// read the live computed token values so the colors track theme + light/dark.
function docgenMermaidVars() {
  try {
    var cs = getComputedStyle(document.documentElement);
    var pick = function (name, fallback) {
      var v = cs.getPropertyValue(name).trim();
      return v || fallback;
    };
    var surface = pick('--surface', '#15140f');
    var text = pick('--text', '#ecebe5');
    var textDim = pick('--text-dim', '#a8a59a');
    return {
      // The rect behind edge labels — match the card so the pill disappears.
      edgeLabelBackground: surface,
      // Keep label + node text legible on that surface.
      labelColor: text,
      nodeTextColor: text,
      titleColor: text,
      lineColor: textDim,
    };
  } catch (e) {
    return {};
  }
}

window.docgen.island('docgenMermaid', function (Alpine) {
  Alpine.data('docgenMermaid', function () {
    return {
      // Original diagram source, stashed on first render so a theme change can
      // re-render from the pristine source (mermaid.render consumes the node).
      _src: null,
      _observer: null,
      async render() {
        const el = this.$el;
        const src = el.querySelector('.docgen-mermaid__src');
        const out = el.querySelector('.docgen-mermaid__out');
        if (!src || !out) return;
        // Stash the source once; subsequent re-renders reuse it.
        if (this._src === null) this._src = src.textContent;
        // docgen.loadScript caches by URL, so multiple diagrams fetch the lib once.
        // Prefix with the deployed base (set by the page) so a sub-path deploy works.
        var base = window.DOCGEN_BASE || '';
        await window.docgen.loadScript(base + '/vendor/mermaid/mermaid.min.js');
        const mermaid = window.mermaid;
        // Re-initialize each render so a theme change takes effect.
        mermaid.initialize({
          startOnLoad: false,
          theme: docgenMermaidTheme(),
          themeVariables: docgenMermaidVars(),
        });
        const id = el.dataset.mermaidId || 'm-' + Math.random().toString(36).slice(2);
        // mermaid.render requires a unique node id per call; bump a suffix so a
        // re-render after a theme switch does not collide with the first.
        this._n = (this._n || 0) + 1;
        try {
          const result = await mermaid.render(id + '-svg-' + this._n, this._src);
          out.innerHTML = result.svg;
        } catch (e) {
          out.innerHTML = '<pre class="docgen-mermaid__error"></pre>';
          out.firstChild.textContent = String(e);
        }
        // Wire a one-time observer that re-renders this diagram whenever the
        // <html> data-theme attribute flips (the theme-toggle island mutates it).
        if (!this._observer && typeof MutationObserver !== 'undefined') {
          var self = this;
          var last = docgenMermaidTheme();
          this._observer = new MutationObserver(function () {
            var now = docgenMermaidTheme();
            if (now === last) return;
            last = now;
            // Re-render from the stashed source with the new theme.
            self.render();
          });
          try {
            this._observer.observe(document.documentElement, {
              attributes: true,
              attributeFilter: ['data-theme'],
            });
          } catch (e) {}
        }
      },
    };
  });
});
