import { readable } from 'svelte/store';

function currentPath(): string {
  if (typeof location === 'undefined') return '/';
  return location.pathname;
}

/**
 * Moves focus to the page's heading after a navigation, so a screen reader announces the new
 * page and the next Tab starts from its top -- a SPA swap otherwise leaves focus on a control
 * that no longer exists (the body, in practice), or on one the old page left behind. A field the
 * new page focused itself (Search's `autofocus`) keeps it. The heading is TopBar's `h1`, which
 * carries `tabindex="-1"` for this. True when it moved focus.
 */
export function focusPageHeading(doc: Document = document): boolean {
  const heading = doc.querySelector<HTMLElement>('main h1');
  if (!heading) return false;
  const active = doc.activeElement as HTMLElement | null;
  if (active && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName)) return false;
  heading.focus({ preventScroll: true });
  return true;
}

export const path = readable<string>(currentPath(), (set) => {
  if (typeof window === 'undefined') return;
  const on = () => {
    set(currentPath());
    // After the new route has rendered (Svelte flushes in a microtask) and applied any
    // `autofocus` of its own. Both `go` and the browser's back/forward land here.
    setTimeout(() => focusPageHeading(), 0);
  };
  window.addEventListener('popstate', on);
  return () => window.removeEventListener('popstate', on);
});

export function go(p: string, replace = false): void {
  if (replace) history.replaceState(null, '', p);
  else history.pushState(null, '', p);
  window.dispatchEvent(new PopStateEvent('popstate'));
  window.scrollTo(0, 0);
}

export function back(fallback = '/'): void {
  if (history.length > 1) history.back();
  else go(fallback, true);
}

/** `/objects/:id` against `/objects/42` → `{ id: '42' }`; null when it does not match. */
export function match(pattern: string, p: string): Record<string, string> | null {
  const norm = (s: string) => (s.length > 1 ? s.replace(/\/+$/, '') : s);
  const pp = norm(pattern).split('/');
  const xs = norm(p).split('/');
  if (pp.length !== xs.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < pp.length; i++) {
    if (pp[i].startsWith(':')) params[pp[i].slice(1)] = decodeURIComponent(xs[i]);
    else if (pp[i] !== xs[i]) return null;
  }
  return params;
}
