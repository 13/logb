export type CounterUnit = 'km' | 'mi' | 'h' | null;
export const CATEGORIES = ['maintenance', 'repair', 'purchase', 'inspection', 'modification', 'fuel', 'other'] as const;
export type Category = (typeof CATEGORIES)[number];
export type Kind = 'photo' | 'document';

export interface User { id: number; username: string; is_admin: boolean; lang: string; created_at?: string }
export interface Settings { currency: string }

export interface ObjectStats { total_cost_cents: number; activity_count: number; current_counter: number | null; due_reminder_count: number }
export interface MemObject {
  id: number; user_id: number; name: string; category: string; counter_unit: CounterUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived_at: string | null;
  cover_attachment_id: number | null; created_at: string; updated_at: string; stats: ObjectStats;
}
export interface ObjectInput {
  name: string; category: string; counter_unit: CounterUnit; description: string;
  purchase_date: string | null; purchase_price_cents: number | null; archived?: boolean; cover_attachment_id?: number | null;
}

export interface Attachment {
  id: number; object_id: number; activity_id: number | null; file_id: number; kind: Kind; caption: string; created_at: string;
  original_name: string; mime: string; size: number; width: number | null; height: number | null; taken_at: string | null;
}

export interface Activity {
  id: number; object_id: number; date: string; category: Category; title: string; notes: string;
  counter_value: number | null; cost_cents: number | null; created_at: string; updated_at: string; attachments: Attachment[];
}
export interface ActivityInput { date: string; category: Category; title: string; notes: string; counter_value: number | null; cost_cents: number | null }

export interface Reminder {
  id: number; object_id: number; title: string; notes: string; due_date: string | null; due_counter: number | null;
  repeat_months: number | null; repeat_counter: number | null; done_at: string | null; done_activity_id: number | null;
  created_at: string; object_name: string; counter_unit: CounterUnit; current_counter: number | null; due: boolean;
}
export interface ReminderInput { title: string; notes: string; due_date: string | null; due_counter: number | null; repeat_months: number | null; repeat_counter: number | null }
export interface DoneOut { done: Reminder; next: Reminder | null }
export interface ImportCounts { objects: number; activities: number; attachments: number; reminders: number }
