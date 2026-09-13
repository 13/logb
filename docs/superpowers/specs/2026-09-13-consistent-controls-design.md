# Consistent controls, and a dashboard that earns its width

Status: implemented. Supersedes `2026-09-11-ui-system-and-states-design.md`
and `2026-09-11-ui-screen-restructure-design.md`, which split this work across two slices and
never named the controls.

## Problem

The app looks unfinished, and the reasons are specific rather than a matter of taste.

**The search box is wearing Chrome's clothes.** `Search.svelte:86` styles it with
`input[type='search'] { width: 100% }` and nothing else. It is the one input in the app that
does not pick up `.field input`'s styling, so it falls through to the browser: square corners
against rounded ones everywhere else, a hard focus rectangle, and a **blue ✕** — the native
clear button, in a colour that appears nowhere else in LogB.

**The dashboard's `+` is a second card.** Each row is a card, and beside it sits a full-height
box holding a `+`, with a gap on either side. It reads as two cards, not as a row with an
action, and it is unlabelled, so it does not say what it does. Meanwhile the card itself holds a
name and one word, leaving most of the row empty. Below three objects there is a screen and a
half of nothing.

**Values escaped the scale.** The design-foundations slice built a type and space scale; the
component `<style>` blocks still hold `6px`, `10px`, `18px`, `.8rem`, `.9rem`. Nothing enforces
it, so gaps differ by a few pixels between screens, which is visible without being nameable.

**Section labels are tracked-out uppercase** (`Search.svelte:87`) — a treatment used nowhere
else in the app.

## What this is not

Not Tailwind, and not shadcn-svelte. Both were measured rather than dismissed:

- shadcn-svelte's runtime costs **~27 KB gzipped for the first component** (measured here: a
  Svelte-only control build is 10.7 KB gzipped, the same build with one `bits-ui` dialog and
  `tailwind-merge` is 37.6 KB), against a current JS bundle of 50 KB gzipped. It is a copy-in
  library, so it retrofits nothing: ~105 control usages would be replaced by hand. Its CLI
  assumes SvelteKit; this app is plain Vite.
- Tailwind ships nothing at runtime and would make the scale the path of least resistance, which
  is a real benefit. But its Preflight reset **removes default styling from form controls**, so
  every native control would be less styled than today until each is dressed — the fix being a
  further dependency or a hand-written base layer, which is exactly the work below.

Either may be right later. Neither fixes what is wrong today, and both would arrive tangled up
with a rewrite.

## Approach

**Style controls by element, not by class.** The cause of the search box is that styling was
opt-in: a control is styled when someone remembers to wrap it in `.field`. Moving the base
styles onto element selectors — `input`, `select`, `textarea`, `button`, `dialog`, and the
checkbox and radio types — means a control is styled *because it exists*. A new screen cannot
forget.

Native chrome is suppressed deliberately rather than inherited: `appearance: none` on the
controls that carry it, including `::-webkit-search-cancel-button`, whose replacement is an
icon button using the app's own `Icon.svelte` and its own colours.

**The dashboard row becomes one object.** The card holds the type icon, the name, and what the
app already knows and currently hides: the counter reading, the total spent, and whether a
reminder is due. The quick-log action lives inside the row rather than beside it, and says what
it does. `Show archived` becomes a filter chip, matching the chips object detail already uses.

**Spacing stops being a matter of memory.** Every stray value resolves to the scale. Where a
literal genuinely encodes something the scale cannot say — an optical alignment, a hairline — it
stays, with a comment saying why. A token scale that cannot express a thing is not improved by a
token that lies about it.

**Screens have an appearance when they are empty.** Dashboard, timeline, documents, reminders
and search each get a state that says what belongs there and offers the action, rather than
stopping at a heading.

## The palette does not change

Teal, off-white, the bee. This is a craft pass, not a new identity. If a different visual
language is wanted later, it is a separate decision and a cheaper one once the bespoke CSS is
gone.

## Testing

Visual work is not unit-testable, and asserting on computed CSS proves only that the file says
what the file says. What is worth testing is what a regression would silently remove:

- **A guard against values escaping the scale.** A test walks every `.svelte` file and fails on a
  raw font size, or a spacing value that is not on the 4px grid, inside a `<style>` block — the
  same shape as the test that keeps emoji out of the icons. Documented exceptions are listed by
  the exact value, so an exception that stops being needed fails loudly.
- **Every control is styled.** A test asserts that an `input`, a `select` and a `textarea`
  rendered outside any `.field` wrapper still carry the app's border radius and height — which
  is the defect that produced the search box.
- **The search box has no native chrome.** Asserted on the rendered control, not on the source.
- **Each empty state renders its action, and the action works** — one end-to-end test per screen,
  seeded with nothing rather than mocked.

Everything else is verified by eye against screenshots in both themes.
