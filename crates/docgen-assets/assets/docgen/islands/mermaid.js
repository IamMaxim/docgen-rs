// Mermaid island: renders each .docgen-mermaid container by lazy-loading the
// vendored mermaid.min.js exactly once, only on pages where a container exists.
// Registered through the docgen.island convention (see bootstrap.js); classic
// script, no ESM imports (the no-npm discipline).
window.docgen.island('docgenMermaid', function (Alpine) {
  Alpine.data('docgenMermaid', function () {
    return {
      async render() {
        const el = this.$el;
        const src = el.querySelector('.docgen-mermaid__src');
        const out = el.querySelector('.docgen-mermaid__out');
        if (!src || !out) return;
        // docgen.loadScript caches by URL, so multiple diagrams fetch the lib once.
        await window.docgen.loadScript('/vendor/mermaid/mermaid.min.js');
        const mermaid = window.mermaid;
        mermaid.initialize({ startOnLoad: false });
        const id = el.dataset.mermaidId || 'm-' + Math.random().toString(36).slice(2);
        try {
          const result = await mermaid.render(id + '-svg', src.textContent);
          out.innerHTML = result.svg;
        } catch (e) {
          out.innerHTML = '<pre class="docgen-mermaid__error"></pre>';
          out.firstChild.textContent = String(e);
        }
      },
    };
  });
});
