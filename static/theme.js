'use strict';

// Loaded before styles so a saved dark theme is applied before the first paint.
window.fieldnotesTheme = (() => {
  const key = 'fieldnotes.theme';
  const themes = {
    fieldnotes: {label: 'Fieldnotes', color: '#284b39', description: 'Warm paper, soft greens, and a little room to grow.'},
    arcade: {label: 'Night Arcade', color: '#0b101b', description: 'A midnight grow space with neon mint, violet accents, and a pixel-grid backdrop.'},
  };
  const normalize = value => Object.hasOwn(themes, value) ? value : 'fieldnotes';
  const current = () => normalize(document.documentElement.dataset.theme);
  function apply(value) {
    const theme = normalize(value);
    document.documentElement.dataset.theme = theme;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', themes[theme].color);
    const select = document.querySelector('#theme-select');
    if (select) select.value = theme;
    const description = document.querySelector('#theme-description');
    if (description) description.textContent = themes[theme].description;
    return theme;
  }
  let saved;
  try { saved = localStorage.getItem(key); } catch (_) { /* Storage may be disabled. */ }
  apply(saved);
  window.addEventListener('storage', event => {
    if (event.key === key || event.key === null) apply(event.newValue);
  });
  return {
    themes,
    current,
    select(value) {
      const theme = apply(value);
      try { localStorage.setItem(key, theme); return true; }
      catch (_) { return false; }
    },
  };
})();
