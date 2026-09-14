export type CounterUnit = 'km' | 'mi' | 'h' | null;
export const CATEGORIES = ['maintenance', 'repair', 'purchase', 'inspection', 'modification', 'fuel', 'other',
  'symptom', 'treatment', 'appointment', 'medication', 'reading'] as const;
export type Category = (typeof CATEGORIES)[number];
export const OBJECT_TYPES = ['car', 'e_bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body', 'other'] as const;
export type ObjectType = (typeof OBJECT_TYPES)[number];
export type Kind = 'photo' | 'document';

export interface User { id: number; username: string; is_admin: boolean; lang: string; created_at?: string }
/** `timezone` is the instance's IANA zone; `timezone_locked` is true when `LOGB_TIMEZONE` sets it. */
export interface Settings { currency: string; timezone: string; timezone_locked: boolean }
/** `GET /me/notifications`: where this person's digest goes. */
export interface NotificationSettings {
  url: string | null; format: 'text' | 'json'; push_devices: number; vapid_public_key: string;
  instance_webhook: boolean; hour: number;
}
export interface NotificationTest { webhook: string | null; push_sent: number; push_failed: number }

export interface ObjectStats {
  total_cost_cents: number; activity_count: number; current_counter: number | null; due_reminder_count: number;
  /** The date of the newest entry with a counter value. */
  last_reading_date: string | null;
  /** The newest entry dated today or earlier. */
  last_activity_date: string | null;
  /** Counter units per day over recent readings, ×1000; null until there is enough history. */
  counter_per_day_milli: number | null;
}
export type FuelUnit = 'l' | 'gal' | 'kwh' | null;
export interface MemObject {
  id: number; user_id: number; name: string; type: ObjectType; counter_unit: CounterUnit; fuel_unit: FuelUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived_at: string | null;
  cover_attachment_id: number | null; cover_file_id: number | null; parent_id: number | null; created_at: string; updated_at: string; stats: ObjectStats;
  /** The chain from the root down to this object's parent, nearest last. A single-object read
   *  fills this in; a list response leaves it out, so it is optional rather than empty. */
  ancestors?: { id: number; name: string }[];
  tags: string[];
}
export interface ObjectInput {
  name: string; type: ObjectType; counter_unit: CounterUnit; fuel_unit: FuelUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived?: boolean; cover_attachment_id?: number | null;
  parent_id?: number | null; tags?: string[];
}

export interface Attachment {
  id: number; object_id: number; activity_id: number | null; file_id: number; kind: Kind; caption: string; created_at: string;
  original_name: string; mime: string; size: number; width: number | null; height: number | null; taken_at: string | null;
  /** Set client-side only, for a synthetic entry built from a still-queued outbox op -- the
   *  server never sends this field. See `FilePicker.svelte`. */
  pending?: boolean;
  /** Client-side-only preview for a `pending` attachment: there is no `file_id` yet (the
   *  server has never seen this file), so this is an object URL made straight from the picked
   *  File instead of a `fileUrl(file_id)` call. */
  previewUrl?: string;
}

export interface Activity {
  id: number; object_id: number; date: string; category: Category; title: string; notes: string;
  counter_value: number | null; cost_cents: number | null; quantity_milli: number | null; created_at: string; updated_at: string; attachments: Attachment[];
  /** Set client-side only, for a synthetic entry built from a still-queued outbox op — the
   *  server never sends this field. See `pendingToActivity` in ObjectDetail.svelte. */
  pending?: boolean;
  tags: string[];
}
export interface ActivityInput { date: string; category: Category; title: string; notes: string; counter_value: number | null; cost_cents: number | null; quantity_milli: number | null; tags?: string[] }

/** `GET /tags`: every distinct tag in use, with how many objects and entries carry it. */
export interface TagCount { tag: string; count: number }

export interface TitleSuggestion {
  title: string; category: Category; last_date: string;
  last_cost_cents: number | null; last_counter: number | null;
}

/** `service` watches a date or a counter target and is marked done; `reading` asks for a counter
 *  reading every `every_n` `every_unit`s and is satisfied by logging one. */
export type ReminderKind = 'service' | 'reading';
export type EveryUnit = 'week' | 'month';
export interface Reminder {
  id: number; object_id: number; title: string; notes: string; due_date: string | null; due_counter: number | null;
  repeat_months: number | null; repeat_counter: number | null; done_at: string | null; done_activity_id: number | null;
  created_at: string; snoozed_until: string | null; object_name: string; counter_unit: CounterUnit; current_counter: number | null; due: boolean;
  days_until: number | null; counter_until: number | null;
  kind: ReminderKind; every_n: number | null; every_unit: EveryUnit | null;
  /** Date of the object's newest counter reading on or before today. */
  last_reading_date: string | null;
  /** When it comes due by date: `due_date` for a service reminder, derived for a reading one. */
  next_due_date: string | null;
  /** A counter target's projected date from recent usage; never makes it due. */
  estimated_due_date: string | null;
}
export interface ReminderInput {
  title: string; notes: string; due_date: string | null; due_counter: number | null; repeat_months: number | null; repeat_counter: number | null;
  kind: ReminderKind; every_n: number | null; every_unit: EveryUnit | null;
}
export interface DoneOut { done: Reminder; next: Reminder | null }
export interface ImportCounts { objects: number; activities: number; attachments: number; reminders: number }

export interface ActivityHit {
  id: number; object_id: number; object_name: string; date: string; category: Category; title: string;
  notes: string; counter_value: number | null; cost_cents: number | null;
}
/** An object hit carries the name of the object it sits inside, so a list of four things
 *  called "Filter" can be told apart without opening any of them. `null` is a root object.
 *
 *  Search sends the object row and `parent_name`, and nothing else -- no `stats`, no
 *  `cover_file_id` (see `ObjectHit` in `src/api/search.rs`). So this is deliberately not a
 *  `MemObject`: claiming those fields would license handing a hit to `ObjectCard`, which reads
 *  `object.stats.due_reminder_count` and would throw on the first one. */
export type ObjectHit = Omit<MemObject, 'stats' | 'cover_file_id'> & { parent_name: string | null };
export interface SearchResults { objects: ObjectHit[]; activities: ActivityHit[] }

export interface Bucket { bucket: string; cost_cents: number; count: number }
export interface Insights {
  by_year: Bucket[]; by_category: Bucket[];
  counter_span: { from: number; to: number } | null;
  cost_per_counter_milli: number | null;
  fuel: { unit: string; quantity_milli: number; per_100_milli: number | null; cost_per_counter_milli: number | null;
    /** Consumption per fill, oldest first, up to twelve. */
    fills: { date: string; per_100_milli: number }[] } | null;
  /** Counter units per day over recent readings, ×1000; null until there is enough history. */
  counter_per_day_milli: number | null;
  /** The last twelve months, oldest first; `amount` is null for a month the readings cannot measure. */
  usage_by_month: { month: string; amount: number | null }[];
  /** Whether the object has any non-deleted child. */
  has_contents: boolean;
  /** Running costs plus purchase prices counted; `per_year_cents` null under 90 days owned. */
  ownership: { total_cents: number; purchase_cents: number; since: string; per_year_cents: number | null };
  /** Twelve months ending with the current one, oldest first, zeros included. */
  by_month: Amount[];
}

/** `GET /stats`. `bucket` is `YYYY`, `YYYY-MM`, an object type, a category, or `purchase_price`. */
export interface Amount { bucket: string; cost_cents: number }
/** `cost_cents` includes every descendant's spend. */
export interface StatsObject { id: number; name: string; type: ObjectType; archived: boolean; cost_cents: number; children: StatsObject[] }
export interface Stats {
  total_cents: number;
  /** Every year with spend, newest first, whichever year is selected. */
  years: string[];
  over_time: Amount[]; by_object: StatsObject[]; by_type: Amount[]; by_category: Amount[];
}

/** An API token as it is listed: never the token itself, which the server returns exactly once
 *  at creation and stores only as a hash. */
export interface ApiToken {
  id: number; name: string; prefix: string; created_at: string; last_used_at: string | null;
}

/** Where a database is, said in a way that can be put on screen. The server builds this from a
 *  redacted URL and never sends the URL itself, so there is no user, password or query string
 *  here to leak back out through the UI. */
export interface DbLocation { backend: 'sqlite' | 'postgres'; host: string | null; database: string | null }
/** `GET /database`: the location in use, flattened, plus whether Settings may change it. */
export interface DbDescription extends DbLocation {
  pointer_writable: boolean;
  /** The database a pointer file names when it is not the one being served -- a switch has
   *  happened and the restart has not. */
  pending: DbLocation | null;
}
/** `POST /database/test`: what another database is, without changing it. */
export interface DbProbe {
  reachable: boolean; version: string | null;
  state: 'empty' | 'holds_logb_data' | 'unreachable';
  message?: string;
}
/** `GET /database/backup`: who is responsible for backing this database up. `scheduled` is
 *  SQLite with a directory configured, `off` is SQLite without one, and `not_ours` is
 *  PostgreSQL -- where LogB's nightly snapshot (`VACUUM INTO`) never runs, whatever
 *  `LOGB_BACKUP_DIR` is set to. `directory`, `hour` and `last_at` are filled in for
 *  `scheduled` only, and `last_at` is null until the first snapshot has been written. */
export interface BackupStatus {
  state: 'scheduled' | 'off' | 'not_ours';
  directory: string | null; last_at: string | null; hour: number | null;
}
/** `POST /database/switch`: the copy report, returned only once the copy has been verified. */
export interface DbSwitched {
  tables: { table: string; rows: number }[];
  epoch: string; pointer: string; restart_required: boolean; database: DbLocation;
}
