ALTER TABLE parsed_paper_artifacts ADD COLUMN figure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE parsed_paper_artifacts ADD COLUMN table_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_mode TEXT NOT NULL DEFAULT 'disabled';
ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_summary_count INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS parsed_visual_artifacts (
  paper_id TEXT PRIMARY KEY,
  version INTEGER NOT NULL,
  asset_dir TEXT NOT NULL,
  figure_count INTEGER NOT NULL DEFAULT 0,
  table_count INTEGER NOT NULL DEFAULT 0,
  visual_mode TEXT NOT NULL DEFAULT 'disabled',
  multimodal_interpreted INTEGER NOT NULL DEFAULT 0,
  warnings_json TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);