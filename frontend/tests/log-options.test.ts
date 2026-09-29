import { describe, expect, it } from 'vitest';
import { logOptions } from '../src/lib/log-options';

const labels = { activity: 'Log activity', trip: 'Log trip', resource: 'Log fill' };

describe('logOptions', () => {
  it('offers only an activity when the object has nothing else', () => {
    expect(logOptions({ oid: 7, offersTrip: false, resourceCategory: null, labels })).toEqual([
      { id: 'activity', path: '/objects/7/activities/new', label: 'Log activity' },
    ]);
  });

  it('lists activity, then fill or charge, then trip', () => {
    expect(logOptions({ oid: 7, offersTrip: true, resourceCategory: 'fuel', labels }).map((o) => [o.id, o.path])).toEqual([
      ['activity', '/objects/7/activities/new'],
      ['resource', '/objects/7/activities/new?category=fuel'],
      ['trip', '/objects/7/activities/new?category=trip'],
    ]);
  });

  it('leaves out what the object does not offer', () => {
    expect(logOptions({ oid: 3, offersTrip: true, resourceCategory: null, labels }).map((o) => o.id)).toEqual(['activity', 'trip']);
  });
});
