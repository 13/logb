import { describe, expect, it } from 'vitest';
import { files, householdData, neverCached, otherApi, thumbnails, type RouteMatch } from '../src/lib/sw-routes';

const at = (path: string, sameOrigin = true) => ({ url: new URL(`https://logb.example${path}`), sameOrigin });

/** The first rule that matches, in the order vite.config.ts registers them. */
function ruleFor(path: string, sameOrigin = true, rules: RouteMatch[] = [neverCached, thumbnails, files, householdData, otherApi]): number {
  return rules.findIndex((r) => r(at(path, sameOrigin)));
}

const CASES: Array<[string, number]> = [
  ['/api/auth/me', 0], ['/api/auth/status', 0], ['/api/auth/login', 0],
  ['/api/export', 0], ['/api/export?object_id=3', 0], ['/api/import', 0],
  ['/api/sync/pull?since=0', 0], ['/api/search?q=golf', 0], ['/api/settings', 0],
  ['/api/users', 0], ['/api/database', 0], ['/api/tokens', 0], ['/api/me/notifications', 0],
  ['/api/stats?year=2026', 0], ['/api/health', 0],
  ['/api/files/12', 2], ['/api/files/12?thumb=1', 2], ['/api/files/12/thumb', 1],
  ['/api/objects', 3], ['/api/objects?all=true&archived=false', 3], ['/api/objects/7', 3],
  ['/api/objects/7/insights', 3], ['/api/objects/7/activities?limit=20', 3], ['/api/activities/9', 3],
  ['/api/objects/7/energy', 3],
  ['/api/stats/energy', 3], ['/api/stats/fuel', 3], ['/api/stats/water', 3],
  ['/api/reminders/due?within_days=30', 3], ['/api/types', 3], ['/api/tags', 3],
  ['/api/something-new', 4],
];

describe('service worker routes', () => {
  it.each(CASES)('%s goes to rule %i', (path, rule) => {
    expect(ruleFor(path)).toBe(rule);
  });

  it('matches nothing on another origin', () => {
    for (const [path] of CASES) expect(ruleFor(path, false)).toBe(-1);
  });

  it('matches nothing outside /api', () => {
    expect(ruleFor('/objects/7')).toBe(-1);
    expect(ruleFor('/apiary')).toBe(-1);
  });

  it('is self-contained, because the service worker receives each matcher as source text', () => {
    const copies = [neverCached, thumbnails, files, householdData, otherApi].map(
      (fn) => new Function(`return (${fn.toString()})`)() as RouteMatch,
    );
    for (const [path, rule] of CASES) expect(ruleFor(path, true, copies)).toBe(rule);
  });
});
