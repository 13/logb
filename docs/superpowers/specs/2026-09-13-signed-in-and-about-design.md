# Who is signed in, signing out, and About

Status: implemented.

## Problem

- Nothing in the app said who was signed in. The only place the username appeared was a muted
  line on Settings > Account.
- Signing out took three steps: Settings, Account, Sign out.
- On a phone the version sat under the last row of every screen, just above the tab bar, and
  said nothing else about the build.
- Sign-in and first-run setup rendered in the app's 720px content column, pinned to the top,
  with their two fields stretched across it.

## Changes

**`SignedIn.svelte`**: an initial, "Signed in as" + the username (and an Admin chip), and a
Sign out button. No confirmation: signing out loses nothing, and writes still queued offline are
tagged with the account and replay on its next sign-in. Used in two places:

- The desktop sidebar's foot, compact (icon-only button), above the version. Visible on every
  screen, one click.
- The top of the Settings hub, full. On a phone that is one tap from the tab bar.

**No footer on a phone.** The in-content `.app-footer` is gone. Settings gains an **About** block:
version, build date and time (`__BUILD_DATE__`), commit (`__BUILD_COMMIT__`, from
`LOGB_BUILD_COMMIT` or `git rev-parse`, omitted when neither is available — the Docker build takes
it as a build arg), and the server's own version from `/api/health` with the database backend for
admins. When the server's version differs from the bundle's — a service worker still serving the
previous release — a hint says to reload.

**`main.auth`**: sign-in and setup become a 400px column centred both ways (`min-height: 100dvh`,
so a phone keyboard scrolls rather than clips), headings and intro centred, full-width submit.

## Tests

`tests-e2e/13-shell.spec.ts`: the sidebar shows the name, version and a working Sign out on
desktop; on mobile no version is on screen outside Settings, and Settings shows who is signed in
and signs out; About shows version, build and server; the sign-in form is horizontally centred,
at most 400px wide, and not pinned to the top. Specs that click the Account page's "Sign out"
are scoped to `main`, since desktop now has a second one in the sidebar.
