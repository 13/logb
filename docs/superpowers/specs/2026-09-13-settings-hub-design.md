# Settings: a hub, not a scroll

Status: approved, not yet implemented. **Depends on `2026-09-13-app-shell-design.md`, which must
be implemented first.**

## The problem

`Settings.svelte` is 456 lines rendering eleven `<h2>` sections in one flat scroll:

| # | Section | Audience |
|---|---------|----------|
| 1 | Failed sync operations | everyone, only when something broke |
| 2 | Language | everyone |
| 3 | Theme | everyone |
| 4 | Account — password, sign out, sign out everywhere | everyone |
| 5 | Currency | admin |
| 6 | Users | admin |
| 7 | New user | admin |
| 8 | Database — switch backend, test, restart | admin |
| 9 | Backup status | admin |
| 10 | API tokens | everyone |
| 11 | Data — export, import | everyone |

The visual vocabulary underneath is good: tokens for spacing, type and colour, plus `.card`,
`.field`, `.list`, `.chips`, and a `--control: 48px` height that every control already honours.
The problem is not how any one section looks. It is that all eleven sit at exactly one level of
hierarchy:

- A personal theme preference has the same visual weight as *migrate this instance to
  PostgreSQL*.
- Destructive, irreversible operations sit inline between benign preferences.
- Everything an administrator can break is interleaved with everything an ordinary user can
  safely touch, so neither audience can scan for what is theirs.
- One 456-line component holds eleven unrelated concerns, which makes every future change to any
  one of them a change to a file nobody can hold in their head.

## What this builds

`/settings` becomes a hub: a short list of rows, each opening its own screen. Six sub-pages,
each small and about exactly one thing.

## The hub

```
Settings
┌─────────────────────────────────────┐
│ ⚠ 2 changes could not be saved    → │   only when it happens
└─────────────────────────────────────┘

 YOU
┌─────────────────────────────────────┐
│ 🎨  Appearance          Dark · EN  →│
│ 👤  Account                   ben  →│
│ 🔑  API access             2 keys  →│
│ 📦  Your data                      →│
└─────────────────────────────────────┘

 THIS INSTANCE                          admin only
┌─────────────────────────────────────┐
│ 👥  People                 3 users →│
│ 🗄  Database         PostgreSQL    →│
└─────────────────────────────────────┘
```

### Rows carry their current value

This is the idea doing most of the work. A row reading `Dark · EN`, `PostgreSQL`, or `2 keys`
answers the question without being opened. The hub becomes a status summary rather than a menu —
"which database is this instance on, how many tokens are live, what theme am I in" are all
answered at a glance.

Values per row:

| Row | Value shown | When absent |
|-----|-------------|-------------|
| Appearance | theme name · language code, e.g. `Dark · EN` | never absent |
| Account | the signed-in username | never absent |
| API access | `n keys`, or nothing when there are none | no tokens: no value |
| Your data | nothing — it is a pair of actions, not a state | always |
| People | `n users` | admin only |
| Database | the backend name, e.g. `PostgreSQL` | until `GET /database` answers: no value |

A value that is not yet loaded renders as nothing at all, never as a placeholder or a guess. The
row is still tappable while its value is pending.

### Grouping

Two groups with quiet headings: **You** (Appearance, Account, API access, Your data) and **This
instance** (People, Database), the second rendered only for administrators. The split is by
*what the thing is*, and it happens to align with who may change it.

### The failed-sync banner

Failed sync operations are not a setting. They are an alert, they appear only when something
actually broke, and they are the one time-sensitive thing on the screen. They stay at the top of
the hub as a banner styled as a problem — not as a row in a group — and the banner is absent
entirely when the queue is clean.

The banner expands **in place** on the hub, showing the same per-operation retry and discard
controls the current Settings screen already renders. It gets no route of its own: a dead
operation is transient and rare, and a permanent URL for a screen that is empty almost always
would be a seventh destination that exists to be blank.

## The six pages

Each is its own route and its own component, reached from the hub and returning to it via
`TopBar`'s back arrow.

### `/settings/appearance`
Language, theme, and — for administrators only — currency.

Currency lives here, not with the server configuration, because it is a display format. Filing a
three-letter currency code next to *migrate this instance to PostgreSQL* would be filing by who
may change it rather than by what it is. It carries a hint saying it applies to everyone on the
instance, so an administrator is not misled into thinking it is a personal preference.

### `/settings/account`
The signed-in username, change password, sign out, and sign out everywhere — with the existing
hint that changing a password already ends every other session.

### `/settings/api`
The token list, creating a token, revoking one, and the one-time display of a freshly created
token's plaintext. This page keeps the existing behaviour exactly: the plaintext is shown once,
never persisted, and dropped when the user leaves.

### `/settings/data`
Export and import, with the existing note that an export is a useful copy but is not a backup of
the database.

### `/settings/people` (admin)
The user list and adding a user.

### What a non-administrator sees at an admin route

The two admin pages are not offered on the hub to a non-administrator, but a URL can still be
typed. Visiting `/settings/people` or `/settings/database` without `is_admin` redirects to
`/settings` — the same shape as the route guards `App.svelte` already applies for `/login` and
`/setup`. This is a convenience, not the security boundary: the endpoints behind these pages are
already administrator-only server-side, and they stay that way. A redirect that was somehow
bypassed would still reach a server that refuses.

### `/settings/database` (admin)
The current database, the connection field, test, switch, the restart prompt, **and the backup
status**.

Database and backup are one page, not two. The backup section exists precisely to say that on
PostgreSQL, LogB backs nothing up — and an instance migrated from SQLite by the controls
immediately above looks, in every other respect, exactly as healthy as it did before. Splitting
them is how somebody migrates and never reads the consequence. All the existing warning
treatments on this page — the blob warning, the restart prompt, the "backed up elsewhere"
card — are kept verbatim, because each was written against a specific way an operator can lose
data.

## Destructive actions

Every irreversible action on these pages — sign out everywhere, revoke a token, delete a user,
switch the database, restart — keeps the confirmation it has today and keeps the project's
existing `danger` treatment. None of them sits adjacent to a benign control without visual
separation. This spec changes where these actions live, never how carefully they are guarded.

## Icons

The hub needs seven icons that `Icon.svelte` does not have: appearance, account, API access,
data, people, database, and a chevron for the row affordance. They are added to the existing
`IconName` union as real SVG paths. The project's icon test forbids emoji and fullwidth glyph
stand-ins, and that guard stays satisfied.

## Desktop

The hub's rows become a two-column grid above the shell's 900px breakpoint; the sub-pages render
in the content pane at the shell's form width. No third navigation column: the shell already
provides a persistent sidebar, and a second permanent nav column for six pages would be heavier
than what it organises.

## Testing

- **Unit (vitest):** the hub's row model is a pure function — given the session, settings, token
  count, user count and database description, produce the rows with their values and their
  visibility. Tests cover: a non-admin sees four rows and no "This instance" group; an admin sees
  six; a pending database description yields a row with no value rather than a placeholder; zero
  tokens yield no value.
- **End-to-end (Playwright), on both the mobile and desktop projects the shell adds:** the hub
  lists its rows; each row navigates to its page; a non-admin reaches `/settings/database`
  directly and is not shown administrative controls; the failed-sync banner appears only when the
  queue holds a dead operation.
- **No regression:** every existing behaviour moved from `Settings.svelte` keeps its current
  tests. Where a test drove the old single-page Settings, it is updated to drive the page the
  behaviour now lives on — moved, not deleted. A behaviour that loses its test during this move
  is a behaviour this redesign silently dropped.

## Out of scope

- Any change to what these settings *do*. This is a reorganisation and a visual redesign; every
  endpoint, payload, and guard stays as it is.
- New settings. Nothing is added that does not exist today.
- The shell itself — navigation, footer, and desktop container are the other spec, and this one
  assumes they already exist.
