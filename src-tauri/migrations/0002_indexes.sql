CREATE INDEX IF NOT EXISTS idx_papers_source ON papers(source);
CREATE INDEX IF NOT EXISTS idx_papers_year ON papers(year);
CREATE INDEX IF NOT EXISTS idx_library_items_updated_at ON library_items(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runs_paper_created_at ON agent_runs(paper_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_workflow_states_paper_id ON workflow_states(paper_id);
CREATE INDEX IF NOT EXISTS idx_paper_parse_tasks_updated_at ON paper_parse_tasks(updated_at DESC);
