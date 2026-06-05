// docgen theme-toggle island. A segmented light/dark control that flips the
// `data-theme` attribute on <html> and persists to localStorage. The pre-paint
// inline script in <head> already set the attribute before first paint, so this
// island only mirrors that state and reacts to clicks. No ESM / no bundler.
window.docgen.island('docgenThemeToggle', function (Alpine) {
  Alpine.data('docgenThemeToggle', function () {
    return {
      theme: document.documentElement.getAttribute('data-theme') || 'dark',
      set: function (t) {
        this.theme = t;
        document.documentElement.setAttribute('data-theme', t);
        try {
          localStorage.setItem('doc-theme', t);
        } catch (e) {}
      },
      toggle: function () {
        this.set(this.theme === 'dark' ? 'light' : 'dark');
      },
    };
  });
});
