/**
 * The look every form control and its text share. In one module so the controls that draw their
 * own label (TagInput, TypeTiles, Segmented) match `Field` exactly.
 */

/** A text control: 48 px tall (the 44 px target plus its border), the `input` token as its
 *  outline (>= 3:1 on page and card), the full-strength ring. `scroll-my-24` keeps a focused
 *  field clear of the sticky top bar and the sticky Save bar. */
export const controlClass =
  'block h-12 w-full min-w-0 scroll-my-24 rounded-lg border border-input bg-card px-3 text-base text-foreground placeholder:text-muted-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring aria-invalid:border-destructive disabled:cursor-not-allowed disabled:opacity-50';
export const labelClass = 'm-0 text-sm font-medium text-foreground';
export const hintClass = 'm-0 text-sm text-muted-foreground';
export const warnClass = 'm-0 text-sm text-warn';
export const errorClass = 'm-0 text-sm font-medium text-destructive';
/** A heading inside a form: well under the page title, the same as the object page's. */
export const sectionHeadingClass = 'm-0 text-xs font-semibold tracking-wide text-muted-foreground uppercase';
/** A one-tap suggestion (repeat an entry, start from a template), in a row that scrolls sideways. */
export const chipClass =
  'inline-flex min-h-11 shrink-0 cursor-pointer items-center whitespace-nowrap rounded-full border border-border bg-card px-3 text-sm text-foreground transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring';
/** A ghost `Button` whose action removes something (Discard, Remove, Revoke, Delete): destructive
 *  text that stays destructive on hover, where the ghost variant would turn it foreground. Its hover
 *  fill (muted, muted/50 in dark, over a card) is contrast-tested in theme-contrast.test.ts. */
export const destructiveGhostClass = 'text-destructive hover:text-destructive';
