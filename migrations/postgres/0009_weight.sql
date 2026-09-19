ALTER TABLE activities ADD COLUMN weight_grams BIGINT CHECK (weight_grams > 0);
ALTER TABLE objects ADD COLUMN weight_unit TEXT NOT NULL DEFAULT 'kg' CHECK (weight_unit IN ('kg','lb'));
ALTER TABLE activities DROP CONSTRAINT activities_category_check;
ALTER TABLE activities ADD CONSTRAINT activities_category_check CHECK (category IN
('maintenance','repair','purchase','inspection','modification','fuel','other','symptom','treatment','appointment','medication','reading','trip','weight'));
CREATE INDEX idx_activities_weight ON activities(object_id, date DESC, created_at DESC, id DESC) WHERE deleted_at IS NULL AND weight_grams IS NOT NULL;
