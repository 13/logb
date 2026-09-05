import { describe, it, expect } from 'vitest';
import { match } from '../src/lib/router';

describe('match', () => {
  it('matches static and param routes', () => {
    expect(match('/', '/')).toEqual({});
    expect(match('/objects/:id', '/objects/42')).toEqual({ id: '42' });
    expect(match('/objects/:id/activities/:aid', '/objects/1/activities/7')).toEqual({ id: '1', aid: '7' });
  });
  it('rejects non-matching paths', () => {
    expect(match('/objects/:id', '/objects')).toBeNull();
    expect(match('/objects/:id', '/objects/1/edit')).toBeNull();
    expect(match('/', '/login')).toBeNull();
  });
  it('ignores a trailing slash', () => {
    expect(match('/login', '/login/')).toEqual({});
  });
});
