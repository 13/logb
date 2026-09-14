# Notifications per person, push, and a round of polish

Status: implemented. Follows the 0.6.1 review of "what else would you improve".

## Notifications

**Per person.** `users` gains `notify_url` and `notify_format`. At the daily tick a user with a
webhook of their own gets a digest of their own reminders; the instance webhook
(`LOGB_NOTIFY_URL`) gets everyone else's, as before. A user's own webhook may be any http(s) URL,
private addresses included, because a self-hosted ntfy on the home network is the likeliest
target; the request is a fixed digest body and nothing it answers is shown to anyone. Errors are
logged `without_url`, since an ntfy topic is a secret.

**In their language.** The digest's words (title, the readings heading, the test message) exist in
English and German in `notify.rs`, chosen by `users.lang`. The app sends its language to the server
whenever it differs from the stored one, so `lang` follows what the person actually reads. The
instance digest uses the first administrator's language.

**Browser push.** `push_subscriptions` holds one row per browser (unique endpoint: re-subscribing
moves it). The VAPID key pair is generated on first use and stored in `settings`, so push works
with no configuration and subscriptions survive restarts. Messages are encrypted to the browser
(`web-push-native`, pure Rust) and carry `{ title, body, url }`; `public/push-sw.js`, pulled into
the generated service worker, shows them and opens `url` on a tap. A push service answering 404 or
410 deletes the subscription. Endpoints must be https (http only on loopback, for tests), so a
signed-in user cannot aim the server at an arbitrary address.

Every target is tried even when an earlier one failed, and failures are reported together. Every
digest is built before the day is marked handled, so a collection failure still retries.

Endpoints: `GET/PUT /me/notifications`, `POST /me/notifications/test`,
`POST/DELETE /push/subscriptions`. UI: Settings > Notifications.

## Timezone

`LOGB_TIMEZONE` is optional. Unset, first-run setup stores the browser's timezone in `settings`,
and an administrator can change it under Appearance; either takes effect without a restart
(`db::TIMEZONE` became an `RwLock`). A set `LOGB_TIMEZONE` wins, and Settings shows it as locked.

## Smaller changes

- **Photos before the title.** Adding files to an untitled entry starts the draft with the
  category as its title, in plain sight in the field, instead of refusing.
- **`stats.last_reading_date`** on every object; the reading form uses it instead of guessing from
  the newest twenty entries.
- **Reminder templates.** A new object's form offers its type's usual reminders, all unticked
  (a car: oil every 15,000 km or 12 months, inspection every 24 months, tyres every 6 months, the
  monthly reading). A distance-based one asks for the current reading, which is also saved as the
  first reading; without it, a template falls back to its date or is skipped.
- **Usage per month.** `insights.usage_by_month` for the last twelve months: a month's highest
  reading minus the highest before it, and unknown -- not zero -- for a month without a reading or
  with a lower one. Drawn under the usage figure on the Info tab.
- **Folded readings.** Two or more consecutive readings in the timeline become one line with the
  date and counter range, expandable.
- **Offline edits.** An activity edit that cannot reach the server is queued with the moment it
  was made (`edited_at`). The server keeps each field only if nothing newer changed it, against the
  same `field_clock` sync uses, capped at its own now so a fast clock cannot win forever. Deletes
  stay online-only.
- **E2E isolation.** `signInFresh` gives a spec a brand-new non-admin user, so data-only specs no
  longer see each other's objects. Specs that need the administrator keep `signIn`.
- **Account menu focus.** Opening it moves focus into the panel; Escape returns it to the avatar;
  tabbing out closes it.

## Schema

SQLite `0012_notifications.sql`, PostgreSQL `0003_notifications.sql`. `copy::TABLES` names
`push_subscriptions` after `users`.
