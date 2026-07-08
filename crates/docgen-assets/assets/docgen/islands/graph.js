// Doc-graph island: renders an SVG force-graph from build-time GraphData JSON.
// Positions are computed in Rust at build time; this island only draws + wires
// interactivity (hover-highlight, click-to-navigate, pan/zoom). No graph lib, no ESM —
// classic script, registered via the docgen.island convention (see bootstrap.js).
window.docgen.island('docgenGraph', function (Alpine) {
  var SVGNS = 'http://www.w3.org/2000/svg';
  function clamp(v, lo, hi) {
    return v < lo ? lo : v > hi ? hi : v;
  }
  Alpine.data('docgenGraph', function () {
    return {
      pan: { x: 0, y: 0 },
      scale: 1,
      hovered: null,
      data: { nodes: [], edges: [] },
      adj: {},
      nodeEls: {},
      nodeMeta: {},
      lineEls: [],
      root: null,
      tip: null,
      init() {
        var tag = document.getElementById('docgen-graph-data');
        if (!tag) return;
        try {
          this.data = JSON.parse(tag.textContent);
        } catch (e) {
          return;
        }
        this.buildAdjacency();
        this.makeTip();
        this.draw();
        this.wirePanZoom();
      },
      makeTip() {
        var tip = document.createElement('div');
        tip.className = 'docgen-graph__tip';
        tip.setAttribute('hidden', '');
        this.$el.appendChild(tip);
        this.tip = tip;
      },
      buildAdjacency() {
        var adj = {};
        var nodes = this.data.nodes || [];
        for (var i = 0; i < nodes.length; i++) adj[nodes[i].slug] = {};
        var edges = this.data.edges || [];
        for (var j = 0; j < edges.length; j++) {
          var f = edges[j].from;
          var t = edges[j].to;
          if (adj[f]) adj[f][t] = true;
          if (adj[t]) adj[t][f] = true;
        }
        this.adj = adj;
      },
      draw() {
        var svg = this.$el.querySelector('svg.docgen-graph__svg');
        if (!svg) return;
        // Idempotent: drop any previously-drawn root so a repeat draw() (e.g. a
        // re-init) can never stack a second graph on top of the first. Each
        // draw() owns exactly one `docgen-graph__root`.
        var stale = svg.querySelectorAll('.docgen-graph__root');
        for (var s = 0; s < stale.length; s++) stale[s].remove();
        // root group carries the pan/zoom transform.
        var root = document.createElementNS(SVGNS, 'g');
        root.setAttribute('class', 'docgen-graph__root');
        this.root = root;

        var bySlug = {};
        var nodes = this.data.nodes || [];
        for (var i = 0; i < nodes.length; i++) bySlug[nodes[i].slug] = nodes[i];

        // edges first so nodes paint on top.
        var gLinks = document.createElementNS(SVGNS, 'g');
        gLinks.setAttribute('class', 'docgen-graph__links');
        var edges = this.data.edges || [];
        this.lineEls = [];
        for (var e = 0; e < edges.length; e++) {
          var a = bySlug[edges[e].from];
          var b = bySlug[edges[e].to];
          if (!a || !b) continue;
          var line = document.createElementNS(SVGNS, 'line');
          line.setAttribute('x1', a.x);
          line.setAttribute('y1', a.y);
          line.setAttribute('x2', b.x);
          line.setAttribute('y2', b.y);
          line._from = edges[e].from;
          line._to = edges[e].to;
          gLinks.appendChild(line);
          this.lineEls.push(line);
        }
        root.appendChild(gLinks);

        var gNodes = document.createElementNS(SVGNS, 'g');
        gNodes.setAttribute('class', 'docgen-graph__nodes');
        this.nodeEls = {};
        var self = this;
        // Prefix node links with the deployed base path so a sub-path deploy
        // (e.g. GitLab Pages at /group/project) navigates correctly. Matches the
        // root-absolute, base-prefixed links the build emits in page templates.
        var base = (typeof window !== 'undefined' && window.DOCGEN_BASE) || '';
        for (var n = 0; n < nodes.length; n++) {
          var node = nodes[n];
          var r = clamp(5 + Math.sqrt(node.degree || 0) * 1.4, 6, 14);
          // anchor so a plain click navigates to /{slug}. Encode per path
          // segment so slugs with spaces/#/?/% produce a well-formed URL that
          // matches the build's directory-emit path.
          var link = document.createElementNS(SVGNS, 'a');
          var href = base + '/' + node.slug.split('/').map(encodeURIComponent).join('/');
          link.setAttributeNS('http://www.w3.org/1999/xlink', 'href', href);
          link.setAttribute('href', href);
          var circle = document.createElementNS(SVGNS, 'circle');
          circle.setAttribute('cx', node.x);
          circle.setAttribute('cy', node.y);
          circle.setAttribute('r', r);
          var label = document.createElementNS(SVGNS, 'title');
          label.textContent = node.title;
          circle.appendChild(label);
          link.appendChild(circle);
          gNodes.appendChild(link);
          this.nodeEls[node.slug] = circle;
          this.nodeMeta[node.slug] = node;
          (function (slug, n) {
            circle.addEventListener('pointerenter', function (ev) {
              self.hover(slug);
              self.showTip(n, ev);
            });
            circle.addEventListener('pointermove', function (ev) {
              self.moveTip(ev);
            });
            circle.addEventListener('pointerleave', function () {
              self.clearHover();
              self.hideTip();
            });
          })(node.slug, node);
        }
        root.appendChild(gNodes);
        svg.appendChild(root);
        this.applyTransform();
      },
      hover(slug) {
        this.hovered = slug;
        var nbrs = this.adj[slug] || {};
        for (var s in this.nodeEls) {
          if (!Object.prototype.hasOwnProperty.call(this.nodeEls, s)) continue;
          var c = this.nodeEls[s];
          if (s === slug || nbrs[s]) {
            c.classList.add('active');
            c.classList.remove('dimmed');
          } else {
            c.classList.add('dimmed');
            c.classList.remove('active');
          }
        }
        for (var i = 0; i < this.lineEls.length; i++) {
          var ln = this.lineEls[i];
          if (ln._from === slug || ln._to === slug) ln.classList.add('active');
          else ln.classList.remove('active');
        }
      },
      clearHover() {
        this.hovered = null;
        for (var s in this.nodeEls) {
          if (!Object.prototype.hasOwnProperty.call(this.nodeEls, s)) continue;
          this.nodeEls[s].classList.remove('active', 'dimmed');
        }
        for (var i = 0; i < this.lineEls.length; i++) {
          this.lineEls[i].classList.remove('active');
        }
      },
      showTip(node, ev) {
        var tip = this.tip;
        if (!tip) return;
        // textContent (not innerHTML) so build-time titles/slugs can't inject markup.
        tip.textContent = '';
        var t = document.createElement('div');
        t.className = 'docgen-graph__tip-title';
        t.textContent = node.title || node.slug;
        var p = document.createElement('div');
        p.className = 'docgen-graph__tip-path';
        p.textContent = '/' + node.slug;
        tip.appendChild(t);
        tip.appendChild(p);
        tip.removeAttribute('hidden');
        this.moveTip(ev);
      },
      moveTip(ev) {
        var tip = this.tip;
        if (!tip || tip.hasAttribute('hidden')) return;
        var rect = this.$el.getBoundingClientRect();
        var w = tip.offsetWidth || 220;
        var h = tip.offsetHeight || 60;
        tip.style.left = clamp(ev.clientX - rect.left + 14, 8, rect.width - w - 8) + 'px';
        tip.style.top = clamp(ev.clientY - rect.top + 14, 8, rect.height - h - 8) + 'px';
      },
      hideTip() {
        if (this.tip) this.tip.setAttribute('hidden', '');
      },
      applyTransform() {
        if (!this.root) return;
        this.root.setAttribute(
          'transform',
          'translate(' + this.pan.x + ',' + this.pan.y + ') scale(' + this.scale + ')'
        );
      },
      wirePanZoom() {
        var svg = this.$el.querySelector('svg.docgen-graph__svg');
        if (!svg) return;
        var self = this;
        var dragging = false;
        var last = { x: 0, y: 0 };
        svg.addEventListener('pointerdown', function (ev) {
          // never start a pan on a node link — let the click navigate.
          if (ev.target.closest && ev.target.closest('a')) return;
          dragging = true;
          last = { x: ev.clientX, y: ev.clientY };
          if (svg.setPointerCapture) {
            try {
              svg.setPointerCapture(ev.pointerId);
            } catch (e) {}
          }
          svg.classList.add('docgen-graph--grabbing');
        });
        svg.addEventListener('pointermove', function (ev) {
          if (!dragging) return;
          self.pan.x = clamp(self.pan.x + (ev.clientX - last.x), -2000, 2000);
          self.pan.y = clamp(self.pan.y + (ev.clientY - last.y), -2000, 2000);
          last = { x: ev.clientX, y: ev.clientY };
          self.applyTransform();
        });
        function endDrag() {
          dragging = false;
          svg.classList.remove('docgen-graph--grabbing');
        }
        svg.addEventListener('pointerup', endDrag);
        svg.addEventListener('pointercancel', endDrag);
        svg.addEventListener(
          'wheel',
          function (ev) {
            ev.preventDefault();
            var factor = ev.deltaY < 0 ? 1.1 : 1 / 1.1;
            self.scale = clamp(self.scale * factor, 0.4, 2.5);
            self.applyTransform();
          },
          { passive: false }
        );
      },
    };
  });
});
