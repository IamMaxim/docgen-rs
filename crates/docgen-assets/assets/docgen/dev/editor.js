// Dev-only in-browser markdown editor island. Loads CodeMirror 5 (vendored UMD,
// already present via the dev-injected <script> tags) to edit the current page's
// markdown source, and saves it back through the path-guarded PUT /__docgen/source
// endpoint. A successful save triggers a server rebuild + SSE live-reload.
//
// Registered via the docgen.island convention (see bootstrap.js); never emitted
// by `docgen build` — only injected by the dev server.
window.docgen.island('docgenEditor', function (Alpine) {
  Alpine.data('docgenEditor', function () {
    return {
      open: false,
      cm: null,
      path: '',
      diskHash: '',
      status: '',
      init() {
        var self = this;
        // Start hidden from a single source of truth (the `open` flag): once
        // Alpine strips x-cloak on load, the empty panel would otherwise flash
        // visible until the first Edit click. toggle() drives visibility after.
        this.$el.style.display = 'none';
        document.querySelectorAll('[data-docgen-edit]').forEach(function (b) {
          b.addEventListener('click', function () {
            self.toggle();
          });
        });
      },
      // Map the current page URL (/guide/intro) to its source (guide/intro.md).
      docPath() {
        var p = location.pathname.replace(/^\/+|\/+$/g, '');
        if (p === '') p = 'index';
        return p + '.md';
      },
      async toggle() {
        this.open = !this.open;
        this.$el.style.display = this.open ? 'block' : 'none';
        if (this.open && !this.cm) await this.mount();
      },
      async mount() {
        this.path = this.docPath();
        var r = await fetch(
          '/__docgen/source?path=' + encodeURIComponent(this.path)
        );
        if (!r.ok) {
          this.status = 'load error';
          return;
        }
        var data = await r.json();
        this.diskHash = data.disk_hash || '';
        this.cm = window.CodeMirror(this.$el, {
          value: data.source || '',
          mode: 'markdown',
          lineNumbers: true,
          lineWrapping: true,
        });
        var saveBtn = document.createElement('button');
        saveBtn.textContent = 'Save';
        saveBtn.className = 'docgen-edit-save';
        var self = this;
        saveBtn.addEventListener('click', function () {
          self.save();
        });
        this.$el.appendChild(saveBtn);
      },
      async save() {
        var res = await fetch('/__docgen/source', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            path: this.path,
            source: this.cm.getValue(),
            disk_hash: this.diskHash,
          }),
        });
        if (res.ok) {
          var d = await res.json();
          this.diskHash = d.disk_hash;
          this.status = 'saved';
        } else {
          this.status = 'error';
        }
        // A successful save triggers a server rebuild + SSE reload automatically.
      },
    };
  });
});
