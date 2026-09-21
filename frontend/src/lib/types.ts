import type { IconName } from './icon-names.js';

export type WeightUnit = 'kg' | 'lb';
export type CounterUnit = 'km' | 'mi' | 'h' | null;
export const CATEGORIES = ['maintenance', 'repair', 'purchase', 'inspection', 'modification', 'fuel', 'usage', 'other',
  'symptom', 'treatment', 'appointment', 'medication', 'reading', 'trip', 'weight', 'session'] as const;
export type Category = (typeof CATEGORIES)[number];
export const OBJECT_TYPES = ['car', 'e_bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body', 'other'] as const;
export type BuiltinType = (typeof OBJECT_TYPES)[number];
/** A built-in key, or `custom:<client_uuid>` naming one of the user's own types. */
export type ObjectType = BuiltinType | `custom:${string}`;
/** `GET /types`: one of the signed-in user's own object types. `key` is what `objects.type` holds. */
export interface CustomType {
  id: number; client_uuid: string; key: string; name: string; icon: IconName; categories: Category[];
  counter_unit: CounterUnit; created_at: string; updated_at: string;
}
export type Kind = 'photo' | 'document';

export interface User { id: number; username: string; is_admin: boolean; lang: string; created_at?: string }
/** `timezone` is the instance's IANA zone; `timezone_locked` is true when `LOGB_TIMEZONE` sets it. */
export interface Settings { currency: string; timezone: string; timezone_locked: boolean }
/** `GET /me/notifications`: where this person's digest goes. */
export interface NotificationSettings {
  url: string | null; format: 'text' | 'json'; push_devices: number; vapid_public_key: string;
  instance_webhook: boolean; hour: number; telegram_configured: boolean; telegram_connected: boolean;
  telegram_display_name: string | null; telegram_last_error: string | null;
  telegram_bot_username: string | null; telegram_legacy: boolean;
}
export interface NotificationTest { webhook: string | null; push_sent: number; push_failed: number; telegram: string | null }
export interface TelegramLink { url: string; qr_svg: string; expires_at: string }

export interface ObjectStats {
  latest_weight_grams?: number | null; latest_weight_date?: string | null;
  total_cost_cents: number; activity_count: number; current_counter: number | null; due_reminder_count: number;
  /** The date of the newest entry with a counter value. */
  last_reading_date: string | null;
  /** The newest entry dated today or earlier. */
  last_activity_date: string | null;
  /** Counter units per day over recent readings, ×1000; null until there is enough history. */
  counter_per_day_milli: number | null;
}
export type FuelUnit = 'l' | 'gal' | 'kwh' | null;
export type ResourceUnit = 'l' | 'gal' | 'kwh' | 'm3' | null;
export type ResourceKind = 'electricity' | 'heating_fuel' | 'vehicle_fuel' | 'water' | null;
export type MeasurementMode = 'usage' | 'meter' | null;
export interface MemObject {
  weight_unit?: WeightUnit;
  id: number; user_id: number; name: string; type: ObjectType; counter_unit: CounterUnit; fuel_unit: FuelUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived_at: string | null;
  cover_attachment_id: number | null; cover_file_id: number | null; parent_id: number | null; created_at: string; updated_at: string; stats: ObjectStats;
  /** The chain from the root down to this object's parent, nearest last. A single-object read
   *  fills this in; a list response leaves it out, so it is optional rather than empty. */
  ancestors?: { id: number; name: string }[];
  tags: string[];
  /** Cents per fuel_unit x1000 (0.30 EUR/kWh -> 30000); null unless set. Only meaningful
   *  alongside a fuel_unit -- see ObjectInput's doc comment. */
  energy_price_milli: number | null;
  /** Liquid tank capacity ×1000 in fuel_unit; null for non-tank objects. */
  fuel_capacity_milli?: number | null;
  resource_unit?: ResourceUnit; resource_kind?: ResourceKind; measurement_mode?: MeasurementMode;
  monthly_target_milli?: number | null; low_level_pct?: number | null; private?: number;
  /** Client-side only: this object is durable in the offline outbox and has no server id yet. */
  pending?: boolean;
}
export interface ObjectInput {
  weight_unit?: WeightUnit;
  name: string; type: ObjectType; counter_unit: CounterUnit; fuel_unit: FuelUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived?: boolean; cover_attachment_id?: number | null;
  parent_id?: number | null; tags?: string[];
  /** Three-state on PATCH like cover_attachment_id: omit to keep the current price, null to
   *  clear it, a number to set it. >= 0, and only alongside a fuel_unit (400 otherwise). */
  energy_price_milli?: number | null;
  fuel_capacity_milli?: number | null;
  resource_unit?: ResourceUnit; resource_kind?: ResourceKind; measurement_mode?: MeasurementMode;
  monthly_target_milli?: number | null; low_level_pct?: number | null; private?: boolean;
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
  weight_grams?: number | null;
  id: number; object_id: number; date: string; category: Category; title: string; notes: string;
  counter_value: number | null; cost_cents: number | null; quantity_milli: number | null; created_at: string; updated_at: string; attachments: Attachment[];
  /** Set client-side only, for a synthetic entry built from a still-queued outbox op — the
   *  server never sends this field. See `pendingToActivity` in ObjectDetail.svelte. */
  pending?: boolean;
  tags: string[];
  /** A trip's fields, `null` on every other category. Its end is the existing `counter_value`
   *  above; distance is `counter_value - start_counter`, never sent or stored. */
  start_counter: number | null;
  from_place: string | null;
  to_place: string | null;
  duration_minutes: number | null;
  battery_used_pct: number | null;
  /** Whether this charge (or fill) topped the battery/tank up: 1 only on a fuel entry, 0
   *  otherwise -- never null. Absent on PATCH keeps the stored value. */
  charged_full: number;
  /** Observed remaining liquid-tank level, 0..100; null on other entries. */
  fuel_level_pct?: number | null;
  meter_reading_milli?: number | null; period_start?: string | null; period_end?: string | null;
  estimated?: number; meter_reset?: number;
}
export interface ActivityInput {
  weight_grams?: number | null;
  date: string; category: Category; title: string; notes: string; counter_value: number | null;
  cost_cents: number | null; quantity_milli: number | null; tags?: string[];
  start_counter?: number | null; from_place?: string | null; to_place?: string | null;
  duration_minutes?: number | null; battery_used_pct?: number | null;
  charged_full?: number;
  fuel_level_pct?: number | null;
  meter_reading_milli?: number | null; period_start?: string | null; period_end?: string | null;
  estimated?: number; meter_reset?: number;
}

/** `GET /tags`: every distinct tag in use, with how many objects and entries carry it. */
export interface TagCount { tag: string; count: number }

export interface TitleSuggestion {
  title: string; category: Category; last_date: string;
  last_cost_cents: number | null; last_counter: number | null;
  /** The newest occurrence's trip places -- always null for any other category. */
  last_from_place: string | null; last_to_place: string | null;
}

/** `GET /objects/{id}/last-done`: a title the object has logged more than once (or once,
 *  alongside an open reminder of the same title), with the values of its newest occurrence. */
export interface LastDone {
  title: string; occurrences: number; last_date: string; last_counter: number | null; last_activity_id: number;
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
  /** The object's tags, so a reminder listed away from its object still says what it belongs to. */
  object_tags?: string[];
}
export interface ReminderInput {
  title: string; notes: string; due_date: string | null; due_counter: number | null; repeat_months: number | null; repeat_counter: number | null;
  kind: ReminderKind; every_n: number | null; every_unit: EveryUnit | null;
}
export interface DoneOut { done: Reminder; next: Reminder | null }
export interface ImportCounts {
	objects: number;
	activities: number;
	attachments: number;
	reminders: number;
	types_created: number;
	types_merged: number;
}

export interface ActivityHit {
  id: number; object_id: number; object_name: string; date: string; category: Category; title: string;
  notes: string; counter_value: number | null; cost_cents: number | null; weight_grams: number | null; tags: string[];
  /** Only ever set on a `trip` hit -- see `placesLabel` in ./trip. */
  from_place: string | null; to_place: string | null;
}
/** An object hit carries the name of the object it sits inside, so a list of four things
 *  called "Filter" can be told apart without opening any of them. `null` is a root object.
 *
 *  Search sends the object row and `parent_name`, and nothing else -- no `stats`, no
 *  `cover_file_id` (see `ObjectHit` in `src/api/search.rs`). So this is deliberately not a
 *  `MemObject`: claiming those fields would license handing a hit to `ObjectCard`, which reads
 *  `object.stats.due_reminder_count` and would throw on the first one. */
export type ObjectHit = Omit<MemObject, 'stats' | 'cover_file_id'> & { parent_name: string | null };
export interface SearchResults { objects: ObjectHit[]; activities: ActivityHit[]; has_more?: boolean }

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
  /** The same twelve months as by_month/usage_by_month, oldest first; zero for a month with no trip. */
  trip_distance_by_month: { month: string; distance: number }[];
}

/** `GET /objects/{id}/trip-places`: earlier from/to places of this object's trips, for the
 *  entry form's suggestions -- distinct, most recent trip first, up to 20 each. */
export interface TripPlaces { from: string[]; to: string[] }

/** One period of `GET /objects/{id}/trips/summary` -- see `TripSummary`. */
export interface TripTotals {
  trips: number; distance: number;
  /** distance / trips, rounded down; null with no trips. */
  avg_distance: number | null;
  /** Average speed (km/h or mi/h) x10, over the trips with a duration; null when none has one. */
  speed_x10: number | null;
  /** Distance per 10% battery used, over the trips with a battery figure; null when none has
   *  one, or their combined percentage is 0. */
  distance_per_10pct: number | null;
}
/** `GET /objects/{id}/trips/summary?today=YYYY-MM-DD`: trip counts and distance for this month,
 *  this year and all time. */
export interface TripSummary { month: TripTotals; year: TripTotals; all: TripTotals }

/** "Charge due", part of `EnergyOut`. */
export interface EnergyBattery {
  /** max(0, 100 - the battery percent used by trips after the last full charge, or dated the
   *  same day and starting at or after its counter -- a trip on the charging day itself still
   *  counts, unless it ran before the charge was plugged in). */
  remaining_pct: number;
  /** remaining_pct x (distance / battery percent), summed over every trip that carries both;
   *  null when no trip does. */
  range_left: number | null;
  /** true once remaining_pct drops to 20 or below -- "charge soon". */
  warn: boolean;
}
/** `GET /objects/{id}/energy`: distance and cost per charge, and when to charge next -- see
 *  `domain::energy::energy`. */
export interface EnergyOut {
  /** The object's fuel_unit, unchanged; null on an object with none. */
  unit: string | null;
  /** The object's energy_price_milli, unchanged. */
  price_milli: number | null;
  /** Mean window distance; null with no windows at all (a single window still yields a figure). */
  distance_per_charge: number | null;
  /** Mean of each window's distance per unit, scaled by 1000; null when no window's closing
   *  charge carries an amount. */
  distance_per_unit_milli: number | null;
  /** Mean of each window's cost per counter unit, scaled by 1000 (cents x1000, like other rate
   *  fields); null when no window's cost can be known. */
  cost_per_counter_milli: number | null;
  /** Null with no full charge yet, or when no trip carries a battery percentage. */
  battery: EnergyBattery | null;
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
export interface EnergyUsageMonth { month: string; kwh_milli: number; charges: number; objects: number }
export interface EnergyUsage {
  months: EnergyUsageMonth[];
  current_kwh_milli: number;
  previous_kwh_milli: number;
  target_kwh_milli: number;
}
export interface FuelUsageMonth { month: string; liters_milli: number; gallons_milli: number; charges: number; objects: number }
export interface FuelUsage {
  months: FuelUsageMonth[];
  current_liters_milli: number; previous_liters_milli: number;
  current_gallons_milli: number; previous_gallons_milli: number;
  levels: FuelTankLevel[];
}
export interface FuelTankLevel {
  object_id: number; object_name: string; unit: 'l' | 'gal'; date: string;
  level_pct: number; capacity_milli: number | null; remaining_milli: number | null;
  low: boolean; estimated_days_remaining: number | null;
}
export interface WaterUsageMonth { month: string; liters_milli: number; cost_cents: number; entries: number; estimated: boolean }
export interface WaterObjectUsage {
  object_id: number; object_name: string; liters_milli: number; target_liters_milli: number | null;
  unit: Exclude<ResourceUnit, null>; current_reading_milli: number | null;
}
export interface WaterUsage {
  months: WaterUsageMonth[]; current_liters_milli: number; previous_liters_milli: number;
  current_cost_cents: number; daily_average_liters_milli: number; anomalies: number; objects: WaterObjectUsage[];
}

/** An API token as it is listed: never the token itself, which the server returns exactly once
 *  at creation and stores only as a hash. */
export interface ApiToken {
  id: number; name: string; prefix: string; created_at: string; last_used_at: string | null;
}

/** A fresh QR sign-in code, as `POST /auth/pair` returns it (see `src/api/pairing.rs`). `qr_svg`
 *  is server-rendered SVG for the same `uri` -- never user input, so it is safe to inline as
 *  HTML. `expires_at` is RFC 3339 (`SecondsFormat::Secs`, `Z`-suffixed). */
export interface PairCode {
  code: string; uri: string; qr_svg: string; expires_at: string;
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
 *  `LOGB_BACKUP_DIR` is set to. `stale` means a configured snapshot is older than the
 *  health threshold. `directory`, `hour` and `last_at` are filled in for scheduled states. */
export interface BackupStatus {
  state: 'scheduled' | 'stale' | 'off' | 'not_ours';
  directory: string | null; last_at: string | null; hour: number | null;
}
/** `POST /database/switch`: the copy report, returned only once the copy has been verified. */
export interface DbSwitched {
  tables: { table: string; rows: number }[];
  epoch: string; pointer: string; restart_required: boolean; database: DbLocation;
}
