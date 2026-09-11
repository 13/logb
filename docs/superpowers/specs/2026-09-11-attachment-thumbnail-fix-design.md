# The attachment thumbnail slab

Status: approved design, not yet implemented.

## Problem

Attaching a photo in the activity form makes the form look broken. The page gains a
388×388 empty grey square holding a 64px thumbnail in its corner, and Save and Cancel are
pushed below the fold: page height goes from 839 to 1068 on a Pixel 7, against an 839px
viewport. On a phone the screen fills with grey and the form reads as though it lost its
contents.

No data is lost. Reproduced with every field filled — date, counter, cost, notes — attached a
photo, saved: all four persist and the timeline entry renders correctly. The defect is
entirely visual.

## Cause

`frontend/src/routes/ActivityForm.svelte:304` wraps each attachment in `<div class="thumb">`.
The global rule at `frontend/src/app.css:123` is written for images in a grid:

```css
.thumb { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: var(--radius-sm); background: var(--surface-2); display: block; }
```

Applied to a `<div>` inside `.thumb-strip` (a flex row), `width: 100%` resolves against the
form's width and `aspect-ratio: 1` squares it off. The component's own rule adds only
`position: relative; flex: none`, so nothing constrains it. The 64px image inside comes from
`.thumb-strip img`, which is why the picture is the right size and its container is not.

The collision was recorded during the icons work and deferred; this is that item.

## Fix

Two changes, because either alone leaves the trap armed:

- **Rename the component's wrapper** to `.strip-item`, with its `.pending` and `.pending-chip`
  children following. The wrapper is a positioning context for the pending badge, not a
  thumbnail, and its class should not claim otherwise.
- **Tighten the global rule to `img.thumb`.** Both genuine users are images —
  `ObjectCard.svelte:14` and `Documents.svelte:50` — so nothing else changes, and a future
  `div.thumb` can no longer inherit a grid image's geometry.

## Testing

`tests-e2e/02-lifecycle.spec.ts` already attaches a photo and did not catch this, because it
asserts `.thumb-strip img` has count 1 — the image was always correct.

The guard is a measurement, not a class check: after attaching, the strip item's width must
equal its image's width. That fails at 388-vs-64 today and passes at 64-vs-64 after the fix.
Asserting a literal 64 would pin an unrelated decoration value; asserting the relationship
states the actual requirement, which is that the container fits its contents.

## Out of scope

The activity form's length, the picker's layout, and the thumbnail strip's design. This
restores what the screen was meant to look like; improving it is the screen-restructure slice.
