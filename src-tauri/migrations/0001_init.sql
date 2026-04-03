CREATE TABLE IF NOT EXISTS papers (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  source_paper_id TEXT,
  title TEXT NOT NULL,
  abstract TEXT,
  authors_json TEXT NOT NULL,
  venue TEXT,
  year INTEGER,
  pdf_url TEXT,
  code_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS uploaded_files (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  paper_id TEXT NOT NULL,
  file_name TEXT NOT NULL,
  storage_path TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  parse_status TEXT NOT NULL,
  parse_error_code TEXT,
  parse_error_message TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS user_profiles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL,
  research_field TEXT NOT NULL,
  focus_topic TEXT,
  reading_goal TEXT NOT NULL,
  output_language TEXT NOT NULL,
  experience_level TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS library_items (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  paper_id TEXT NOT NULL,
  status TEXT NOT NULL,
  tags_json TEXT,
  starred INTEGER NOT NULL DEFAULT 0,
  last_read_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, paper_id),
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS model_configs (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  base_url TEXT NOT NULL,
  model_name TEXT NOT NULL,
  api_type TEXT,
  api_key_fallback TEXT,
  agent_type TEXT,
  is_default INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS parsed_paper_artifacts (
  paper_id TEXT PRIMARY KEY,
  version INTEGER NOT NULL,
  storage_path TEXT NOT NULL,
  parser_name TEXT NOT NULL,
  page_count INTEGER,
  section_count INTEGER NOT NULL,
  full_text_available INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS paper_parse_tasks (
  paper_id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  stage TEXT NOT NULL,
  progress INTEGER NOT NULL DEFAULT 0,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  last_error_code TEXT,
  last_error_message TEXT,
  retryable INTEGER,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS workflow_states (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  paper_id TEXT NOT NULL UNIQUE,
  current_step TEXT NOT NULL,
  previous_step TEXT,
  next_action_required TEXT,
  allowed_actions_json TEXT NOT NULL,
  last_failed_run_id TEXT,
  error_code TEXT,
  error_message TEXT,
  retryable INTEGER,
  fallback_actions_json TEXT,
  latest_handoff_summary_ids_json TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS agent_runs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  paper_id TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  status TEXT NOT NULL,
  model_config_id TEXT,
  input_snapshot_json TEXT NOT NULL,
  output_snapshot_json TEXT,
  error_code TEXT,
  error_message TEXT,
  token_usage_json TEXT,
  cost_estimate REAL,
  started_at TEXT,
  finished_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id)
);

CREATE TABLE IF NOT EXISTS agent_handoff_summaries (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  paper_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  stage TEXT NOT NULL,
  compressed_conclusion TEXT NOT NULL,
  key_points_json TEXT NOT NULL,
  carry_forward_questions_json TEXT NOT NULL,
  carry_forward_evidence_json TEXT NOT NULL,
  next_step_suggestion TEXT NOT NULL,
  generated_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (paper_id) REFERENCES papers(id),
  FOREIGN KEY (run_id) REFERENCES agent_runs(id)
);
