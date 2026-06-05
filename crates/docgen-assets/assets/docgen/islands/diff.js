// docgen diff-workspace island. A faithful plain-JS port of DocDiffView.svelte
// (the orchestrator) plus its four sub-components — CommitHeader, DiffTimelineRail,
// DiffFileTree, DiffFileView. No Alpine, no framework: this island owns ALL the
// dynamic workspace markup and renders it imperatively into `#docgen-diff-root`.
//
// Data flow mirrors the Svelte original:
//   * `${base}/diff/timeline.json`            -> DocDiffReport (timeline points carry
//                                                 NO blocks — only summary stats + tree)
//   * `${base}/diff/revisions/<id>.json`      -> a single DocDiffTimelinePoint whose
//                                                 files[].blocks are populated (lazy,
//                                                 cached by id)
//
// The class names + markup match diff.css verbatim so the sibling CSS port styles
// this island unchanged. Text is escaped; block.html is trusted server-rendered
// markdown and is injected raw.
//
// Runs at DOM-ready; registers a no-op island to join the bootstrap registry.
(function () {
  'use strict';

  // ---- config / constants -------------------------------------------------
  var ROOT_ID = 'docgen-diff-root';
  var RAIL_KEY = 'doc-diff-rail-w';
  var FILES_KEY = 'doc-diff-files-w';
  var WIDTH_MIN = 140;
  var WIDTH_MAX = 500;
  var WIDTH_DEFAULT = 200;

  // ---- tiny helpers -------------------------------------------------------
  function clamp(v, lo, hi) {
    return v < lo ? lo : v > hi ? hi : v;
  }

  // HTML-escape text content (titles, paths, subjects). block.html is NOT run
  // through this — it is trusted server-rendered markdown.
  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function el(tag, className) {
    var node = document.createElement(tag);
    if (className) node.className = className;
    return node;
  }

  // ---- port of timeline-groups.ts ----------------------------------------
  function ymd(d) {
    return (
      d.getFullYear() +
      '-' +
      String(d.getMonth() + 1).padStart(2, '0') +
      '-' +
      String(d.getDate()).padStart(2, '0')
    );
  }

  function formatDate(value) {
    if (!value) return '';
    var d = new Date(value);
    if (Number.isNaN(d.getTime())) return '';
    return ymd(d);
  }

  function bucketLabel(point, now) {
    if (point.kind === 'worktree') return 'Working tree';
    var today = ymd(now);
    var yesterday = ymd(new Date(now.getTime() - 86400000));
    var day = point.date ? ymd(new Date(point.date)) : '';
    if (day === today) return 'Today';
    if (day === yesterday) return 'Yesterday';
    return 'Earlier';
  }

  function groupTimeline(points, now) {
    now = now || new Date();
    var buckets = [];
    for (var i = 0; i < points.length; i++) {
      var label = bucketLabel(points[i], now);
      var existing = null;
      for (var j = 0; j < buckets.length; j++) {
        if (buckets[j].label === label) {
          existing = buckets[j];
          break;
        }
      }
      if (existing) existing.points.push(points[i]);
      else buckets.push({ label: label, points: [points[i]] });
    }
    return buckets;
  }

  // ---- port of tree.ts ----------------------------------------------------
  function collectStats(node) {
    if (node.type === 'file') {
      return { files: 1, added: node.addedLines, removed: node.removedLines };
    }
    var files = 0;
    var added = 0;
    var removed = 0;
    for (var i = 0; i < node.children.length; i++) {
      var s = collectStats(node.children[i]);
      files += s.files;
      added += s.added;
      removed += s.removed;
    }
    return { files: files, added: added, removed: removed };
  }

  function flattenTree(nodes, depth, collapsed) {
    var out = [];
    for (var i = 0; i < nodes.length; i++) {
      var node = nodes[i];
      if (node.type === 'file') {
        out.push({
          rowType: 'file',
          id: node.id,
          label: node.label,
          path: node.path,
          oldPath: node.oldPath,
          status: node.status,
          addedLines: node.addedLines,
          removedLines: node.removedLines,
          depth: depth
        });
        continue;
      }
      var stats = collectStats(node);
      var isCollapsed = collapsed.has(node.id);
      out.push({
        rowType: 'group',
        id: node.id,
        label: node.label,
        depth: depth,
        fileCount: stats.files,
        addedLines: stats.added,
        removedLines: stats.removed,
        collapsed: isCollapsed
      });
      if (!isCollapsed) {
        var nested = flattenTree(node.children, depth + 1, collapsed);
        for (var k = 0; k < nested.length; k++) out.push(nested[k]);
      }
    }
    return out;
  }

  function groupBlockRuns(blocks) {
    var runs = [];
    for (var i = 0; i < blocks.length; i++) {
      var block = blocks[i];
      var last = runs[runs.length - 1];
      if (last && last.kind === block.kind) last.blocks.push(block);
      else runs.push({ kind: block.kind, blocks: [block] });
    }
    return runs;
  }

  // ---- shared status glyph (parity with DiffFileTree / DiffFileView) -------
  function statusGlyph(s) {
    return s === 'added' ? 'A' : s === 'modified' ? 'M' : s === 'deleted' ? 'D' : 'R';
  }

  function fileBaseName(path) {
    var i = path.lastIndexOf('/');
    return i === -1 ? path : path.slice(i + 1);
  }
  function fileDirName(path) {
    var i = path.lastIndexOf('/');
    return i === -1 ? '' : path.slice(0, i + 1);
  }

  // =========================================================================
  // The workspace controller. One instance per mount; holds all reactive state
  // and re-renders on change (coarse but cheap — matches the original's feel).
  // =========================================================================
  function DiffWorkspace(root, base) {
    this.root = root;
    this.base = base;
    this.timelineUrl = base + '/diff/timeline.json';
    this.revisionUrlPrefix = base + '/diff/revisions/';

    // state (mirrors the $state fields in DocDiffView)
    this.report = null;
    this.selectedPointId = null;
    this.selectedFilePath = null;
    this.timelineLoading = true;
    this.timelineError = null;
    this.revisionError = null;
    this.loadingRevisionId = null;
    this.revisions = {}; // id -> DocDiffRevision (a point with blocks)
    this.collapsedGroups = new Set();
    this.timelineOpen = false;
    this.lastSyncedUrl = '';

    // persisted resizer widths (localStorage doc-diff-rail-w / doc-diff-files-w)
    this.railWidth = this.readWidth(RAIL_KEY);
    this.filesWidth = this.readWidth(FILES_KEY);

    this.onKey = this.handleKey.bind(this);
  }

  DiffWorkspace.prototype.readWidth = function (key) {
    var raw = null;
    try {
      raw = localStorage.getItem(key);
    } catch (e) {
      /* ignore privacy errors */
    }
    if (raw === null) return WIDTH_DEFAULT;
    var n = parseInt(raw, 10);
    return Number.isFinite(n) && n >= WIDTH_MIN && n <= WIDTH_MAX ? n : WIDTH_DEFAULT;
  };

  DiffWorkspace.prototype.writeWidth = function (key, value) {
    try {
      localStorage.setItem(key, String(value));
    } catch (e) {
      /* ignore quota / privacy errors */
    }
  };

  // ---- derived getters (mirror the $derived blocks) -----------------------
  DiffWorkspace.prototype.selectedSummaryPoint = function () {
    if (!this.report) return null;
    var t = this.report.timeline;
    for (var i = 0; i < t.length; i++) if (t[i].id === this.selectedPointId) return t[i];
    return t[0] || null;
  };

  DiffWorkspace.prototype.selectedPoint = function () {
    if (this.selectedPointId && this.revisions[this.selectedPointId]) {
      return this.revisions[this.selectedPointId];
    }
    return this.selectedSummaryPoint();
  };

  DiffWorkspace.prototype.selectedFile = function () {
    var point = this.selectedPoint();
    if (!point) return null;
    for (var i = 0; i < point.files.length; i++) {
      if (point.files[i].path === this.selectedFilePath) return point.files[i];
    }
    return point.files[0] || null;
  };

  DiffWorkspace.prototype.treeRows = function () {
    var point = this.selectedPoint();
    return point ? flattenTree(point.fileTree, 0, this.collapsedGroups) : [];
  };

  DiffWorkspace.prototype.selectedRevisionLoaded = function () {
    return this.selectedPointId !== null && this.revisions[this.selectedPointId] !== undefined;
  };

  DiffWorkspace.prototype.blockRuns = function () {
    var file = this.selectedFile();
    return groupBlockRuns((file && file.blocks) || []);
  };

  DiffWorkspace.prototype.groupedTimeline = function () {
    return this.report ? groupTimeline(this.report.timeline) : [];
  };

  // ---- lifecycle ----------------------------------------------------------
  DiffWorkspace.prototype.mount = function () {
    window.addEventListener('keydown', this.onKey);
    this.render();
    this.loadTimeline();
  };

  // ---- data loading -------------------------------------------------------
  DiffWorkspace.prototype.loadTimeline = function () {
    var self = this;
    this.timelineLoading = true;
    this.timelineError = null;
    this.render();
    fetch(this.timelineUrl)
      .then(function (r) {
        if (!r.ok) throw new Error(r.status + ' ' + r.statusText);
        return r.json();
      })
      .then(function (report) {
        self.report = report;
        var params = new URL(window.location.href).searchParams;
        var wantCommit = params.get('c');
        var wantFile = params.get('f');
        var matchedPoint = null;
        if (wantCommit) {
          for (var i = 0; i < report.timeline.length; i++) {
            if (report.timeline[i].id === wantCommit) {
              matchedPoint = report.timeline[i];
              break;
            }
          }
        }
        self.selectedPointId =
          (matchedPoint && matchedPoint.id) ||
          report.selectedPointId ||
          (report.timeline[0] && report.timeline[0].id) ||
          null;
        var initialPoint = matchedPoint;
        if (!initialPoint) {
          for (var j = 0; j < report.timeline.length; j++) {
            if (report.timeline[j].id === self.selectedPointId) {
              initialPoint = report.timeline[j];
              break;
            }
          }
        }
        if (!initialPoint) initialPoint = report.timeline[0] || null;
        var matchedFile = null;
        if (wantFile && initialPoint) {
          for (var k = 0; k < initialPoint.files.length; k++) {
            if (initialPoint.files[k].path === wantFile) {
              matchedFile = initialPoint.files[k];
              break;
            }
          }
        }
        self.selectedFilePath =
          (matchedFile && matchedFile.path) ||
          report.selectedFilePath ||
          (initialPoint && initialPoint.files[0] && initialPoint.files[0].path) ||
          null;
      })
      .catch(function (e) {
        self.timelineError = e instanceof Error ? e.message : String(e);
      })
      .then(function () {
        self.timelineLoading = false;
        self.afterStateChange();
      });
  };

  DiffWorkspace.prototype.loadRevision = function (id) {
    var self = this;
    this.loadingRevisionId = id;
    this.revisionError = null;
    fetch(this.revisionUrlPrefix + encodeURIComponent(id) + '.json')
      .then(function (r) {
        if (!r.ok) throw new Error(r.status + ' ' + r.statusText);
        return r.json();
      })
      .then(function (rev) {
        self.revisions[id] = rev;
      })
      .catch(function (e) {
        if (self.selectedPointId === id) {
          self.revisionError = e instanceof Error ? e.message : String(e);
        }
      })
      .then(function () {
        if (self.loadingRevisionId === id) self.loadingRevisionId = null;
        self.afterStateChange();
      });
  };

  // ---- selection / interaction -------------------------------------------
  DiffWorkspace.prototype.selectPoint = function (p) {
    this.selectedPointId = p.id;
    this.selectedFilePath = (p.files[0] && p.files[0].path) || null;
    this.timelineOpen = false;
    this.afterStateChange();
  };

  DiffWorkspace.prototype.selectFile = function (path) {
    this.selectedFilePath = path;
    this.afterStateChange();
  };

  DiffWorkspace.prototype.toggleGroup = function (id) {
    var next = new Set(this.collapsedGroups);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.collapsedGroups = next;
    this.afterStateChange();
  };

  DiffWorkspace.prototype.toggleTimeline = function () {
    this.timelineOpen = !this.timelineOpen;
    this.afterStateChange();
  };

  // Runs the cluster of $effects that the original re-evaluates on every state
  // change: reconcile the selected file, lazy-load the revision, sync the URL,
  // then re-render and (when ready) kick mermaid.
  DiffWorkspace.prototype.afterStateChange = function () {
    var point = this.selectedPoint();

    // $effect: keep selectedFilePath valid for the active point.
    if (point) {
      var has = false;
      for (var i = 0; i < point.files.length; i++) {
        if (point.files[i].path === this.selectedFilePath) {
          has = true;
          break;
        }
      }
      if (!has) this.selectedFilePath = (point.files[0] && point.files[0].path) || null;
    }

    // $effect: lazy-load the revision for the selected point.
    if (this.selectedPointId && !this.revisions[this.selectedPointId]) {
      if (this.loadingRevisionId !== this.selectedPointId) {
        this.loadRevision(this.selectedPointId);
      }
    }

    // $effect: reflect selection into the URL.
    if (this.report) this.syncUrl();

    this.render();

    // $effect: re-hydrate mermaid once the revision + file are rendered.
    if (this.selectedRevisionLoaded() && this.selectedFile()) {
      var self = this;
      Promise.resolve().then(function () {
        window.dispatchEvent(new CustomEvent('docgen-mermaid-rerun'));
        void self;
      });
    }
  };

  DiffWorkspace.prototype.syncUrl = function () {
    var url = new URL(window.location.href);
    if (this.selectedPointId) url.searchParams.set('c', this.selectedPointId);
    else url.searchParams.delete('c');
    if (this.selectedFilePath) url.searchParams.set('f', this.selectedFilePath);
    else url.searchParams.delete('f');
    var next = url.pathname + url.search;
    if (next === this.lastSyncedUrl) return;
    this.lastSyncedUrl = next;
    history.replaceState(history.state, '', next);
  };

  // ---- keyboard nav (port of handleKey + jumpBlock) -----------------------
  DiffWorkspace.prototype.handleKey = function (e) {
    var t = e.target;
    if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return;
    if (t && t.isContentEditable) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    var point = this.selectedPoint();
    var files = (point && point.files) || [];
    var timeline = (this.report && this.report.timeline) || [];
    var selFile = this.selectedFile();
    var fileIdx = -1;
    for (var i = 0; i < files.length; i++) {
      if (selFile && files[i].path === selFile.path) {
        fileIdx = i;
        break;
      }
    }
    var pointIdx = -1;
    for (var j = 0; j < timeline.length; j++) {
      if (point && timeline[j].id === point.id) {
        pointIdx = j;
        break;
      }
    }
    if (e.key === 'j' && files.length) {
      var nf = files[Math.min(files.length - 1, fileIdx + 1)];
      if (nf) this.selectFile(nf.path);
      e.preventDefault();
    } else if (e.key === 'k' && files.length) {
      var pf = files[Math.max(0, fileIdx - 1)];
      if (pf) this.selectFile(pf.path);
      e.preventDefault();
    } else if (e.key === 'J' && timeline.length) {
      var np = timeline[Math.min(timeline.length - 1, pointIdx + 1)];
      if (np) this.selectPoint(np);
      e.preventDefault();
    } else if (e.key === 'K' && timeline.length) {
      var pp = timeline[Math.max(0, pointIdx - 1)];
      if (pp) this.selectPoint(pp);
      e.preventDefault();
    } else if (e.key === ']' || e.key === '[') {
      this.jumpBlock(e.key === ']');
      e.preventDefault();
    }
  };

  DiffWorkspace.prototype.jumpBlock = function (forward) {
    var els = Array.prototype.slice.call(
      document.querySelectorAll('.diff-page .diff-run--added, .diff-page .diff-run--removed')
    );
    if (!els.length) return;
    var y = window.scrollY;
    var sorted = els
      .map(function (e) {
        return { el: e, top: e.getBoundingClientRect().top + y };
      })
      .sort(function (a, b) {
        return a.top - b.top;
      });
    var target;
    if (forward) {
      target =
        sorted.find(function (x) {
          return x.top > y + 4;
        }) || sorted[0];
    } else {
      target =
        sorted
          .slice()
          .reverse()
          .find(function (x) {
            return x.top < y - 4;
          }) || sorted[sorted.length - 1];
    }
    target.el.scrollIntoView({ block: 'start', behavior: 'smooth' });
  };

  // =========================================================================
  // Rendering — imperative builders, one per Svelte component. The .diff-page
  // article is rebuilt wholesale on each render() (cheap; preserves scroll on
  // smooth jumps because we don't touch window position).
  // =========================================================================
  DiffWorkspace.prototype.render = function () {
    var root = this.root;
    root.innerHTML = '';

    var page = el('article', 'diff-page');

    // Warnings / load-error banner (mirrors the {#if} ladder at the top).
    if (this.timelineError) {
      var werr = el('section', 'warnings');
      werr.setAttribute('aria-labelledby', 'diff-load-error');
      werr.innerHTML =
        '<h2 id="diff-load-error">Failed to load timeline</h2><p>' +
        esc(this.timelineError) +
        '</p>';
      page.appendChild(werr);
    } else if (this.report && this.report.warnings.length > 0) {
      var w = el('section', 'warnings');
      w.setAttribute('aria-labelledby', 'diff-warnings');
      var html = '<h2 id="diff-warnings">Warnings</h2><ul>';
      for (var i = 0; i < this.report.warnings.length; i++) {
        html += '<li>' + esc(this.report.warnings[i]) + '</li>';
      }
      html += '</ul>';
      w.innerHTML = html;
      page.appendChild(w);
    }

    if (this.timelineLoading) {
      page.appendChild(this.renderPageSkeleton());
    } else if (this.report && this.report.timeline.length === 0) {
      var empty = el('section', 'empty-state');
      empty.innerHTML = '<p>No Markdown/SVX changes under docs/.</p>';
      page.appendChild(empty);
    } else if (this.report && this.selectedPoint()) {
      page.appendChild(this.renderCommitHeader());
      page.appendChild(this.renderWorkspace());
    }

    root.appendChild(page);
  };

  DiffWorkspace.prototype.renderPageSkeleton = function () {
    var skel = el('div', 'skel skel--page');
    skel.setAttribute('aria-hidden', 'true');
    skel.innerHTML =
      '<div class="skel-rail"><div></div><div></div><div></div><div></div></div>' +
      '<div class="skel-main"><div></div><div></div><div></div></div>';
    return skel;
  };

  // ---- CommitHeader.svelte ------------------------------------------------
  DiffWorkspace.prototype.renderCommitHeader = function () {
    var point = this.selectedPoint();
    var title = point.kind === 'worktree' ? 'Uncommitted changes' : point.subject;
    var hash = point.kind === 'worktree' ? 'worktree' : point.shortHash;
    var dateLabel = formatDate(point.date);

    var header = el('header', 'commit-header');

    var h1 = el('h1');
    h1.title = title;
    h1.textContent = title;
    header.appendChild(h1);

    var meta = el('p', 'commit-meta');
    var metaHtml = '<span class="hash">' + esc(hash) + '</span>';
    if (dateLabel) {
      metaHtml += '<span class="sep" aria-hidden="true">·</span><span>' + esc(dateLabel) + '</span>';
    }
    if (point.author) {
      metaHtml +=
        '<span class="sep" aria-hidden="true">·</span><span>' + esc(point.author) + '</span>';
    }
    metaHtml += '<span class="sep" aria-hidden="true">·</span><span>' + point.files.length + ' files</span>';
    metaHtml += '<span class="added">+' + point.totalAddedLines + '</span>';
    metaHtml += '<span class="removed">−' + point.totalRemovedLines + '</span>';
    meta.innerHTML = metaHtml;
    header.appendChild(meta);

    var btn = el('button', 'timeline-toggle');
    btn.type = 'button';
    btn.setAttribute('aria-expanded', this.timelineOpen ? 'true' : 'false');
    btn.textContent = this.timelineOpen ? 'Hide commits' : 'Switch commit';
    var self = this;
    btn.addEventListener('click', function () {
      self.toggleTimeline();
    });
    header.appendChild(btn);

    return header;
  };

  // ---- .diff-workspace grid -----------------------------------------------
  DiffWorkspace.prototype.renderWorkspace = function () {
    var point = this.selectedPoint();
    var ws = el('div', 'diff-workspace' + (this.timelineOpen ? ' timeline-open' : ''));
    ws.style.setProperty('--rail-w', this.railWidth + 'px');
    ws.style.setProperty('--files-w', this.filesWidth + 'px');

    ws.appendChild(this.renderTimelineRail());
    ws.appendChild(this.renderResizer('Resize timeline rail', RAIL_KEY));
    ws.appendChild(this.renderFileTree(point));
    ws.appendChild(this.renderResizer('Resize file list', FILES_KEY));
    ws.appendChild(this.renderFileView());

    return ws;
  };

  // ---- .col-resizer (port of the `resizable` action) ----------------------
  DiffWorkspace.prototype.renderResizer = function (ariaLabel, key) {
    var self = this;
    var node = el('div', 'col-resizer');
    node.setAttribute('role', 'separator');
    node.setAttribute('aria-orientation', 'vertical');
    node.setAttribute('aria-label', ariaLabel);

    var isRail = key === RAIL_KEY;
    var drag = null;

    node.addEventListener('pointerdown', function (event) {
      drag = {
        pointerId: event.pointerId,
        start: event.clientX,
        startValue: isRail ? self.railWidth : self.filesWidth
      };
      node.setPointerCapture(event.pointerId);
    });
    node.addEventListener('pointermove', function (event) {
      if (!drag || drag.pointerId !== event.pointerId) return;
      var delta = event.clientX - drag.start;
      var next = clamp(drag.startValue + delta, WIDTH_MIN, WIDTH_MAX);
      if (isRail) self.railWidth = next;
      else self.filesWidth = next;
      // Live-update the grid var without a full re-render (matches the action).
      var ws = self.root.querySelector('.diff-workspace');
      if (ws) ws.style.setProperty(isRail ? '--rail-w' : '--files-w', next + 'px');
    });
    var onUp = function (event) {
      if (!drag || drag.pointerId !== event.pointerId) return;
      node.releasePointerCapture(drag.pointerId);
      drag = null;
      self.writeWidth(key, isRail ? self.railWidth : self.filesWidth);
    };
    node.addEventListener('pointerup', onUp);
    node.addEventListener('pointercancel', onUp);

    return node;
  };

  // ---- DiffTimelineRail.svelte --------------------------------------------
  DiffWorkspace.prototype.renderTimelineRail = function () {
    var self = this;
    var point = this.selectedPoint();
    var selectedId = (point && point.id) || null;
    var buckets = this.groupedTimeline();

    var aside = el('aside', 'timeline-rail');
    aside.setAttribute('aria-label', 'Commit timeline');

    var pointTitle = function (p) {
      return p.kind === 'worktree' ? 'Uncommitted changes' : p.subject;
    };
    var pointHash = function (p) {
      return p.kind === 'worktree' ? 'worktree' : p.shortHash;
    };

    for (var b = 0; b < buckets.length; b++) {
      var bucket = buckets[b];
      var h3 = el('h3');
      h3.textContent = bucket.label;
      aside.appendChild(h3);

      var ul = el('ul');
      for (var p = 0; p < bucket.points.length; p++) {
        (function (pt) {
          var li = el('li');
          var btn = el('button');
          btn.type = 'button';
          if (pt.id === selectedId) {
            btn.classList.add('active');
            btn.setAttribute('aria-current', 'true');
          }
          if (pt.kind === 'worktree') btn.classList.add('worktree');
          btn.title = pointTitle(pt);

          var date = formatDate(pt.date);
          btn.innerHTML =
            '<span class="dot" aria-hidden="true"></span>' +
            '<span class="copy"><strong>' +
            esc(pointTitle(pt)) +
            '</strong><small>' +
            esc(pointHash(pt) + (date ? ' · ' + date : '')) +
            '</small></span>' +
            '<span class="stat"><span class="added">+' +
            pt.totalAddedLines +
            '</span><span class="removed">−' +
            pt.totalRemovedLines +
            '</span></span>';
          btn.addEventListener('click', function () {
            self.selectPoint(pt);
          });
          li.appendChild(btn);
          ul.appendChild(li);
        })(bucket.points[p]);
      }
      aside.appendChild(ul);
    }

    return aside;
  };

  // ---- DiffFileTree.svelte ------------------------------------------------
  DiffWorkspace.prototype.renderFileTree = function (point) {
    var self = this;
    var rows = this.treeRows();
    var selFile = this.selectedFile();
    var selectedPath = (selFile && selFile.path) || null;

    var aside = el('aside', 'file-sidebar');
    aside.setAttribute('aria-label', 'Files changed');

    var head = el('div', 'sidebar-head');
    head.innerHTML =
      '<h2>Files</h2><div class="sidebar-summary"><span>' +
      point.files.length +
      '</span><span class="added">+' +
      point.totalAddedLines +
      '</span><span class="removed">−' +
      point.totalRemovedLines +
      '</span></div>';
    aside.appendChild(head);

    var tree = el('div', 'file-tree');
    tree.setAttribute('role', 'tree');
    tree.setAttribute('aria-label', 'Changed files');

    for (var i = 0; i < rows.length; i++) {
      (function (row) {
        if (row.rowType === 'group') {
          var gbtn = el('button', 'tree-row tree-row--group');
          gbtn.type = 'button';
          gbtn.setAttribute('role', 'treeitem');
          gbtn.setAttribute('aria-expanded', row.collapsed ? 'false' : 'true');
          gbtn.setAttribute('aria-selected', 'false');
          gbtn.setAttribute('style', '--depth:' + row.depth + ';');
          gbtn.innerHTML =
            '<span class="chev" aria-hidden="true">' +
            (row.collapsed ? '▸' : '▾') +
            '</span><span class="tree-label">' +
            esc(row.label) +
            '</span><span class="tree-counts"><span class="count-files">' +
            row.fileCount +
            '</span><span class="added">+' +
            row.addedLines +
            '</span><span class="removed">−' +
            row.removedLines +
            '</span></span>';
          gbtn.addEventListener('click', function () {
            self.toggleGroup(row.id);
          });
          tree.appendChild(gbtn);
        } else {
          var fbtn = el('button', 'tree-row tree-row--file');
          fbtn.type = 'button';
          if (row.path === selectedPath) {
            fbtn.classList.add('active');
            fbtn.setAttribute('aria-current', 'true');
          }
          fbtn.setAttribute('role', 'treeitem');
          fbtn.setAttribute('aria-selected', row.path === selectedPath ? 'true' : 'false');
          fbtn.setAttribute('style', '--depth:' + row.depth + ';');
          fbtn.title = row.path;
          var counts = '';
          if (row.addedLines) counts += '<span class="added">+' + row.addedLines + '</span>';
          if (row.removedLines) counts += '<span class="removed">−' + row.removedLines + '</span>';
          counts +=
            '<span class="pill pill--' +
            esc(row.status) +
            '" title="' +
            esc(row.status) +
            '">' +
            esc(statusGlyph(row.status)) +
            '</span>';
          fbtn.innerHTML =
            '<span class="tree-label">' +
            esc(row.label) +
            '</span><span class="tree-counts">' +
            counts +
            '</span>';
          fbtn.addEventListener('click', function () {
            self.selectFile(row.path);
          });
          tree.appendChild(fbtn);
        }
      })(rows[i]);
    }

    aside.appendChild(tree);
    return aside;
  };

  // ---- DiffFileView.svelte ------------------------------------------------
  DiffWorkspace.prototype.renderFileView = function () {
    var file = this.selectedFile();
    var revisionLoaded = this.selectedRevisionLoaded();
    var runs = this.blockRuns();

    var main = el('main', 'selected-file');

    if (file) {
      main.appendChild(this.renderFileHeader(file));

      if (revisionLoaded) {
        if (runs.length === 0) {
          var noBlocks = el('section', 'empty-state');
          noBlocks.innerHTML = '<p>No paragraph-level changes in this file.</p>';
          main.appendChild(noBlocks);
        } else {
          main.appendChild(this.renderRenderedBlocks(runs));
        }
      } else if (this.revisionError) {
        var rerr = el('section', 'empty-state');
        rerr.innerHTML = '<p>Failed to load revision diff: ' + esc(this.revisionError) + '</p>';
        main.appendChild(rerr);
      } else {
        var skel = el('div', 'skel skel--blocks');
        skel.setAttribute('aria-hidden', 'true');
        skel.innerHTML = '<div></div><div></div><div></div>';
        main.appendChild(skel);
      }
    } else {
      var sel = el('section', 'empty-state');
      sel.innerHTML = '<p>Select a file from the sidebar.</p>';
      main.appendChild(sel);
    }

    var hint = el('p', 'kbd-hint');
    hint.innerHTML =
      '<kbd>j</kbd>/<kbd>k</kbd> file · <kbd>J</kbd>/<kbd>K</kbd> commit · ' +
      '<kbd>[</kbd>/<kbd>]</kbd> jump block';
    main.appendChild(hint);

    return main;
  };

  DiffWorkspace.prototype.renderFileHeader = function (file) {
    var header = el('header', 'file-header');

    var pill = el('span', 'pill pill--' + file.status);
    pill.title = file.status;
    pill.textContent = statusGlyph(file.status);
    header.appendChild(pill);

    var path = el('span', 'path');
    if (file.oldPath && file.oldPath !== file.path) {
      path.innerHTML =
        '<span class="path-dir">' +
        esc(fileDirName(file.oldPath)) +
        '</span><span class="path-base">' +
        esc(fileBaseName(file.oldPath)) +
        '</span><span class="arrow" aria-hidden="true">→</span><span class="path-dir">' +
        esc(fileDirName(file.path)) +
        '</span><span class="path-base">' +
        esc(fileBaseName(file.path)) +
        '</span>';
    } else {
      path.innerHTML =
        '<span class="path-dir">' +
        esc(fileDirName(file.path)) +
        '</span><span class="path-base">' +
        esc(fileBaseName(file.path)) +
        '</span>';
    }
    header.appendChild(path);

    header.appendChild(el('span', 'spacer'));

    var counts = el('span', 'counts');
    counts.innerHTML =
      '<span class="added">+' +
      file.addedLines +
      '</span><span class="removed">−' +
      file.removedLines +
      '</span>';
    header.appendChild(counts);

    return header;
  };

  DiffWorkspace.prototype.renderRenderedBlocks = function (runs) {
    var wrap = el('div', 'rendered-blocks doc-shell doc-content');
    for (var ri = 0; ri < runs.length; ri++) {
      var run = runs[ri];
      var runEl = el('div', 'diff-run diff-run--' + run.kind);

      var gutter = el('div', 'diff-gutter');
      gutter.setAttribute('aria-hidden', 'true');
      gutter.innerHTML =
        '<span class="gutter-glyph">' +
        (run.kind === 'added' ? '+' : run.kind === 'removed' ? '−' : '·') +
        '</span>';
      runEl.appendChild(gutter);

      var body = el('div', 'diff-body');
      for (var bi = 0; bi < run.blocks.length; bi++) {
        var block = el('section', 'diff-block');
        block.setAttribute('aria-label', run.kind);
        // block.html is trusted server-rendered markdown — inject raw.
        block.innerHTML = run.blocks[bi].html;
        body.appendChild(block);
      }
      runEl.appendChild(body);
      wrap.appendChild(runEl);
    }
    return wrap;
  };

  // =========================================================================
  // Bootstrapping
  // =========================================================================
  function init() {
    var root = document.getElementById(ROOT_ID);
    if (!root) return; // page has no diff workspace — do nothing.
    var base = window.DOCGEN_BASE || '';
    var ws = new DiffWorkspace(root, base);
    ws.mount();
  }

  function ready(fn) {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', fn);
    } else {
      fn();
    }
  }

  ready(init);

  // Register a no-op island so it joins the bootstrap registry if present.
  if (window.docgen && window.docgen.island) {
    window.docgen.island('docgenDiff', function () {});
  }
})();
