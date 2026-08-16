// Theme preflight. Loaded as a BLOCKING <script src> in <head>, before the
// stylesheets, so the resolved theme (and saved rail width) land on <html>
// before first paint — no flash of the wrong theme. It must stay synchronous
// (no defer/async): deferring it would let the browser paint with the default
// theme first. It is an external file rather than an inline snippet so pages
// run under a strict Content-Security-Policy (script-src 'self').
(function () {
  try {
    var s = localStorage.getItem('doc-theme');
    var t = s || (matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark');
    document.documentElement.setAttribute('data-theme', t);
    var w = parseInt(localStorage.getItem('doc-left-rail-width'), 10);
    if (w >= 180 && w <= 560) document.documentElement.style.setProperty('--left-rail-width', w + 'px');
  } catch (e) {}
})();
