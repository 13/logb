export type ThemePref = 'auto' | 'light' | 'dark';

/** Whether the dark palette applies: a fixed choice wins, "auto" follows the system. */
export function isDark(pref: ThemePref, systemDark: boolean): boolean {
  return pref === 'dark' || (pref === 'auto' && systemDark);
}

/**
 * Switches the palette (app.css keys it on `data-theme`) and paints the browser's own bar --
 * `<meta name="theme-color">` -- in the page background of that palette, read back from
 * app.tw.css's `--ui-background` so the colour has one source. index.html ships one meta per system scheme for the
 * first paint; once the app knows the actual theme (which may differ from the system's, when
 * chosen in Settings) both carry it, unconditionally.
 */
export function applyTheme(doc: Document, dark: boolean): void {
  doc.documentElement.dataset.theme = dark ? 'dark' : 'light';
  const bg = doc.defaultView?.getComputedStyle(doc.documentElement as Element).getPropertyValue('--ui-background').trim();
  if (!bg) return;
  for (const meta of doc.querySelectorAll('meta[name="theme-color"]')) {
    meta.removeAttribute('media');
    meta.setAttribute('content', bg);
  }
}

/**
 * Applies `pref` now and, on "auto", again whenever the system setting changes while the app is
 * open (a phone switching to dark at sunset). Returns the cleanup.
 */
export function followTheme(pref: ThemePref, systemDark: MediaQueryList, apply: (dark: boolean) => void): () => void {
  const update = () => apply(isDark(pref, systemDark.matches));
  update();
  if (pref !== 'auto') return () => {};
  systemDark.addEventListener('change', update);
  return () => systemDark.removeEventListener('change', update);
}
