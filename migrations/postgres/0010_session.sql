ALTER TABLE activities DROP CONSTRAINT activities_category_check;
ALTER TABLE activities ADD CONSTRAINT activities_category_check CHECK (category IN
('maintenance','repair','purchase','inspection','modification','fuel','other','symptom','treatment','appointment','medication','reading','trip','weight','session'));
