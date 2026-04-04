ALTER TABLE model_configs ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE model_configs ADD COLUMN is_recent INTEGER NOT NULL DEFAULT 0;

UPDATE model_configs
SET display_name = CASE
  WHEN TRIM(display_name) = '' THEN provider || ' / ' || model_name
  ELSE display_name
END;

UPDATE model_configs
SET is_recent = 1
WHERE id IN (
  SELECT id
  FROM model_configs
  ORDER BY updated_at DESC
  LIMIT 1
);