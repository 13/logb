import { describe, it, expect, vi } from 'vitest';
import { focusPageHeading, match } from '../src/lib/router';

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

describe('focusPageHeading', () => {
  /** Just enough of a document: the page heading and whatever currently has focus. */
  function fakeDoc(active: { tagName: string } | null, withHeading = true) {
    const heading = { tagName: 'H1', focus: vi.fn() };
    const doc = {
      activeElement: active,
      body: { tagName: 'BODY' },
      querySelector: vi.fn((sel: string) => (withHeading && sel === 'main h1' ? heading : null)),
    };
    return { doc: doc as unknown as Document, heading };
  }

  it("moves focus to the new page's heading, without scrolling", () => {
    const { doc, heading } = fakeDoc({ tagName: 'BODY' });
    (doc as unknown as { activeElement: unknown }).activeElement = doc.body;
    expect(focusPageHeading(doc)).toBe(true);
    expect(heading.focus).toHaveBeenCalledWith({ preventScroll: true });
  });

  it('moves focus off a control the previous page left focused', () => {
    const { doc, heading } = fakeDoc({ tagName: 'BUTTON' });
    expect(focusPageHeading(doc)).toBe(true);
    expect(heading.focus).toHaveBeenCalled();
  });

  it('leaves a field the new page focused itself (autofocus) alone', () => {
    for (const tagName of ['INPUT', 'TEXTAREA', 'SELECT']) {
      const { doc, heading } = fakeDoc({ tagName });
      expect(focusPageHeading(doc)).toBe(false);
      expect(heading.focus).not.toHaveBeenCalled();
    }
  });

  it('does nothing on a page without a heading', () => {
    const { doc } = fakeDoc(null, false);
    expect(focusPageHeading(doc)).toBe(false);
  });
});
